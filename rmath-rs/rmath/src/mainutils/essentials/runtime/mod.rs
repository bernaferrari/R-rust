//! Essentials domain module `runtime` — extracted verbatim from essentials.rs.
//!
//! Split into domain submodules; every path `crate::mainutils::essentials::runtime::*`
//! resolves exactly as before via the glob re-exports below.

mod environment;
mod eval;
mod graphics;
mod matchcall;
mod memory;
mod packages;
mod parallel;
mod rversion;
mod serialize;
mod session;
mod source;
mod sys;
mod typecheck;
mod with;

pub use self::environment::*;
pub use self::eval::*;
pub use self::graphics::*;
pub use self::matchcall::*;
pub use self::memory::*;
pub use self::packages::*;
pub use self::parallel::*;
pub use self::rversion::*;
pub use self::serialize::*;
pub use self::session::*;
pub use self::source::*;
pub use self::sys::*;
pub use self::typecheck::*;
pub use self::with::*;
