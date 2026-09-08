//! Essential R built-in functions — c(), seq(), rep(), paste(), cat(), typeof(), is.na(), names().
//!
//! These are the most fundamental R functions that every R program uses.
//! Split into domain submodules; every public path `crate::mainutils::essentials::*`
//! resolves exactly as before via the glob re-exports below.

use std::ffi::CString;
use std::os::raw::c_int;

#[allow(unused_imports)]
use crate::sexp::accessors::{
    ATTRIB, CADR, CAR, CDR, CHAR, COMPLEX, FORMALS, FRAME, HASHTAB, INTEGER, INTEGER_ELT, LENGTH,
    LOGICAL, LOGICAL_ELT, PRINTNAME, RAW, REAL, REAL_ELT, SET_ENCLOS, SET_OBJECT, SET_STRING_ELT,
    SET_VECTOR_ELT, SETCAR, SETCDR, SETTAG, STRING_ELT, TAG, TYPEOF, VECTOR_ELT, XLENGTH,
};
#[allow(unused_imports)]
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3, Rf_cons, Rf_mkChar,
    Rf_mkString,
};
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals::{R_NilValue, R_UnboundValue};
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

mod conditions;
mod functional;
mod io;
mod mathstats;
mod matrix;
mod print;
mod registry;
mod runtime;
mod s3;
mod s4;
mod sets;
mod shared;
mod strings;
mod tables;
#[cfg(test)]
mod tests;
mod vectors;
pub use self::conditions::*;
pub use self::functional::*;
pub use self::io::*;
pub use self::mathstats::*;
pub use self::matrix::*;
pub use self::print::*;
pub use self::runtime::*;
pub use self::s3::*;
pub use self::s4::*;
pub use self::sets::*;
pub use self::shared::*;
pub use self::strings::*;
pub use self::tables::*;
pub use self::vectors::*;

// ---------------------------------------------------------------------------
// Core vector/scalar helpers live in `essentials_basic`.
// ---------------------------------------------------------------------------

pub use super::essentials_basic::*;

// ---------------------------------------------------------------------------
// Distribution-function builtins live in the `distributions` submodule and are
// re-exported here so registration paths (crate::mainutils::essentials::do_dnorm)
// stay valid. See rport-btb7 for the incremental decomposition plan.
// ---------------------------------------------------------------------------
pub mod distributions;
pub use self::distributions::*;
pub mod environment_bindings;
pub use self::environment_bindings::*;

// ---------------------------------------------------------------------------
// Register essentials builtins
// ---------------------------------------------------------------------------

/// Register essential builtins in the base environment.
pub unsafe fn register_essentials_builtins(env: SEXP) {
    unsafe {
        use crate::sexp::accessors::SET_FRAME;

        let frame = (*env).data.envsxp.frame;
        let mut chain = frame;

        for &name in registry::ALL_FNS {
            // `[` / `[[` are SPECIALSXP upstream (funtab eval=0): they must
            // not be pre-evaluated, so empty subscript slots (`m[,1]`) reach
            // the subset handlers' keep-missing argument evaluation.
            let kind = match name {
                "quote" | "substitute" | "[" | "[[" => SEXPTYPE::SPECIALSXP,
                _ => SEXPTYPE::BUILTINSXP,
            };
            let prim = crate::eval::primitive::make_primitive_binding(name, kind);
            let sym = Rf_install(CString::new(name).unwrap_or_default().as_ptr());
            let cell = Rf_cons(prim, chain);
            (*cell).data.listsxp.tagval = sym;
            chain = cell;
        }
        let pi_sym = Rf_install(c"pi".as_ptr());
        let pi_value = Rf_ScalarReal(std::f64::consts::PI);
        let _pi_value_guard = protect(pi_value);
        let pi_cell = Rf_cons(pi_value, chain);
        (*pi_cell).data.listsxp.tagval = pi_sym;
        chain = pi_cell;

        let letters_value = static_string_vector(&[
            "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q",
            "r", "s", "t", "u", "v", "w", "x", "y", "z",
        ]);
        let _letters_guard = protect(letters_value);
        let letters_cell = Rf_cons(letters_value, chain);
        (*letters_cell).data.listsxp.tagval = Rf_install(c"letters".as_ptr());
        chain = letters_cell;

        let letters_upper_value = static_string_vector(&[
            "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q",
            "R", "S", "T", "U", "V", "W", "X", "Y", "Z",
        ]);
        let _letters_upper_guard = protect(letters_upper_value);
        let letters_upper_cell = Rf_cons(letters_upper_value, chain);
        (*letters_upper_cell).data.listsxp.tagval = Rf_install(c"LETTERS".as_ptr());
        chain = letters_upper_cell;

        let version_value = do_R_version(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            R_NilValue(),
            env,
        );
        let _version_guard = protect(version_value);
        for name in ["R.version", "version"] {
            let sym = Rf_install(CString::new(name).unwrap_or_default().as_ptr());
            let cell = Rf_cons(version_value, chain);
            (*cell).data.listsxp.tagval = sym;
            chain = cell;
        }

        let version_string = do_R_version_string(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            R_NilValue(),
            env,
        );
        let _version_string_guard = protect(version_string);
        let sym = Rf_install(c"R.version.string".as_ptr());
        let cell = Rf_cons(version_string, chain);
        (*cell).data.listsxp.tagval = sym;
        chain = cell;
        SET_FRAME(env, chain);
    }
}
