#![no_std]
#![feature(
    ptr_metadata,
    unsize,
    maybe_uninit_array_assume_init,
    const_maybe_uninit_zeroed,
    strict_provenance,
    unboxed_closures,
    fn_traits
)]
#![deny(unsafe_op_in_unsafe_fn)]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod sexp;
pub mod gc;
pub mod session;
pub mod object;
pub mod symbol;
pub mod env;
pub mod vector;
pub mod promise;
pub mod arena;
pub mod eval;
pub mod error;
pub mod context;

pub use sexp::{Sexp, Tag, TypeTagged, SEXPTYPE};
pub use gc::{Gc, Root, Scope, Trace, WriteBarrier};
pub use session::Session;
pub use object::Object;
