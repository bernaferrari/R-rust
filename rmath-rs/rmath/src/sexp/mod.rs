#![allow(non_snake_case, non_upper_case_globals, unused_variables)]

//! R's S-expression type system.
//!
//! This module provides Rust-native implementations of R's SEXPREC/SEXP types,
//! used throughout the R interpreter. The design follows a two-layer approach:
//! - `ffi` submodule: raw `#[repr(C)]` types for FFI compatibility
//! - internal `globals` storage: global singleton values (R_NilValue, etc.)
//! - internal C-compatible accessor functions (TYPEOF, CAR, CDR, etc.)
//! - `memory` submodule: arena allocator for R objects
//! - internal FFI constructor functions (allocVector, cons, etc.)
//! - `symbol` submodule: symbol table and interning

pub(crate) mod accessors;
#[cfg(feature = "altrep")]
pub mod altrep;
pub mod attrib_core;
pub mod builder;
pub(crate) mod constructors;
pub mod context;
pub(crate) mod env_hash;
pub mod envir;
pub mod ffi;
pub mod gengc;
pub(crate) mod globals;
pub(crate) mod init;
pub(crate) mod instance;
pub mod memory;
pub(crate) mod memory_ext;
pub(crate) mod numeric;
pub mod object;
pub mod output;
pub mod protect;
pub mod session;
pub mod symbol;

// Re-export commonly used types at the module level
pub use ffi::{
    Closxp, DOTSXP, Envsxp, FALSE, ISNAN, Listsxp, NA_INTEGER, NA_LOGICAL, NA_REAL, Primsxp,
    Promsxp, R_FINITE, R_IsNA, R_IsNaN, R_NA_BIT_PATTERN, R_len_t, R_size_t, R_xlen_t, Rboolean,
    Rbyte, Rcomplex, SEXP, SEXPTYPE, SexprecCore, SexprecData, SxpInfo, Symsxp, TRUE, Vecsxp,
};

#[cfg(feature = "altrep")]
pub use altrep::{
    AltrepBuilder, AltrepClass, AltrepData, REPEAT_CLASS, SEQUENCE_CLASS, altrep_as_integer_slice,
    altrep_as_real_slice, altrep_class, altrep_dataptr, altrep_elt, altrep_length,
    force_materialization, is_altrep, is_materialized,
};

pub use output::{
    RCapturedOutput, capture_stderr, capture_stdout, is_capturing, start_capture, stop_capture,
};

pub use instance::SessionCapabilities;
pub use object::{
    PairlistIter, Sexp, SexpAttribute, SexpComplex, SexpError, SexpMetadata, SexpResult, SexpValue,
    SexpView,
};
pub use session::{CancellationToken, RSession};

/// Default-build guards for the ALTREP feature gate.
///
/// `sexp::altrep`, `mainutils::altrep`, and `mainutils::altclasses` are all
/// `#[cfg(feature = "altrep")]`: in the default build they do not exist, and
/// any code referencing them fails to compile. These tests pin the observable
/// side of that contract.
#[cfg(all(test, not(feature = "altrep")))]
mod no_altrep_guards {
    use crate::sexp::accessors::ALTREP;
    use crate::sexp::ffi::{SEXP, SEXPTYPE};
    use crate::sexp::memory::with_arena;

    #[test]
    fn altrep_feature_is_off_in_default_build() {
        assert!(!cfg!(feature = "altrep"));
    }

    /// With every ALTREP constructor compiled out, no public path can set the
    /// ALT bit; a plain VECSXP holding a REALSXP survives a full collection
    /// through the general payload tracing, unmarked and uncorrupted.
    #[test]
    fn default_build_never_produces_altrep_objects() {
        let _session = crate::sexp::session::RSession::new();
        let sym =
            unsafe { crate::sexp::symbol::Rf_install(b"no_altrep_probe\0".as_ptr() as *const _) };
        let outer = with_arena(|arena| arena.alloc_vector(SEXPTYPE::VECSXP, 2));
        let inner = with_arena(|arena| arena.alloc_vector(SEXPTYPE::REALSXP, 4));
        unsafe {
            *((*outer).gengc_next_node as *mut SEXP) = inner;
            crate::sexp::envir::defineVar(sym, outer, crate::sexp::globals::R_GlobalEnv());
        }
        crate::sexp::gengc::full_gc();
        unsafe {
            assert_eq!(ALTREP(outer), 0);
            assert_eq!(ALTREP(inner), 0);
            assert_eq!(*((*outer).gengc_next_node as *mut SEXP), inner);
            assert_eq!(
                crate::sexp::envir::R_findVarInFrame(crate::sexp::globals::R_GlobalEnv(), sym),
                outer
            );
        }
    }
}
