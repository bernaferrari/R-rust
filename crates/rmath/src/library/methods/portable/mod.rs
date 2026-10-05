//! The original pinned GNU methods namespace for runtimes without host R.
use crate::sexp::object::Sexp;

pub(crate) fn namespace() -> Result<Sexp<'static>, String> {
    crate::library::portable_package::image("methods")
        .expect("registered methods image")
        .namespace()
}

pub(crate) fn attach() -> Result<(), String> {
    crate::library::portable_package::image("methods")
        .expect("registered methods image")
        .attach()
}

#[cfg(test)]
mod tests;
