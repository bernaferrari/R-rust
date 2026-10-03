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
pub(crate) mod altseq;
pub mod attrib_core;
pub mod builder;
pub(crate) mod constructors;
pub mod context;
pub(crate) mod env_hash;
pub mod envir;
pub mod ffi;
pub mod gengc;
pub(crate) mod globals;
pub(crate) mod heap;
pub(crate) mod init;
pub(crate) mod instance;
pub mod memory;
pub(crate) mod memory_ext;
pub(crate) mod numeric;
pub mod object;
pub mod output;
pub(crate) mod owner;
pub(crate) mod payload;
pub(crate) mod protect;
pub mod session;
pub mod symbol;

// Re-export commonly used types at the module level
#[allow(unused_imports)]
pub use ffi::{
    Closxp, DOTSXP, Envsxp, FALSE, ISNAN, Listsxp, NA_INTEGER, NA_LOGICAL, NA_REAL, NodeBody,
    Primsxp, Promsxp, R_FINITE, R_IsNA, R_IsNaN, R_NA_BIT_PATTERN, R_len_t, R_size_t, R_xlen_t,
    Rboolean, Rbyte, Rcomplex, SEXP, SEXPTYPE, SexprecCore, SxpInfo, Symsxp, TRUE, Vecsxp,
};

#[cfg(feature = "altrep")]
#[allow(unused_imports)]
pub use altrep::{
    AltrepBuilder, AltrepClass, AltrepClassHandle, AltrepContext, AltrepElement, DeferredClass,
    RepeatClass, SequenceClass, altrep_class, altrep_elt, altrep_length, force_materialization,
    is_altrep, is_materialized,
};

#[allow(unused_imports)]
pub use output::{
    RCapturedOutput, capture_stderr, capture_stdout, is_capturing, start_capture, stop_capture,
};

#[allow(unused_imports)]
pub use instance::SessionCapabilities;
#[allow(unused_imports)]
pub use object::{
    PairlistIter, Sexp, SexpAttribute, SexpComplex, SexpError, SexpMetadata, SexpMut, SexpRef,
    SexpResult, SexpValue,
};
#[allow(unused_imports)]
pub use session::{CancellationToken, RSession};

/// Default-build guards for the ALTREP cargo feature.
///
/// `sexp::altrep` stays behind `altrep`; the native adapter modules require
/// the separate `altrep-native` feature. Compact sequences in [`altseq`] may set the
/// ALT bit; they are ordinary vectors whose formula is a traced attribute.
/// These tests pin the feature gate and that a plain vector still survives
/// collection with the ALT bit clear.
#[cfg(all(test, not(feature = "altrep")))]
mod no_altrep_guards {
    use crate::sexp::accessors::ALTREP;
    use crate::sexp::ffi::SEXPTYPE;
    use crate::sexp::memory::with_arena;

    #[test]
    fn altrep_feature_is_off_in_default_build() {
        assert!(!cfg!(feature = "altrep"));
    }

    /// A plain VECSXP holding a REALSXP survives a full collection with the ALT
    /// bit clear. Compact sequences are a separate path and are not built here.
    #[test]
    fn plain_vector_survives_full_gc_with_alt_bit_clear() {
        let _session = crate::sexp::session::RSession::new_for_gc_tests();
        let sym =
            unsafe { crate::sexp::symbol::Rf_install(b"no_altrep_probe\0".as_ptr() as *const _) };
        let outer = unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| arena.alloc_vector(SEXPTYPE::VECSXP, 2))
        };
        let inner = unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| arena.alloc_vector(SEXPTYPE::REALSXP, 4))
        };
        unsafe {
            crate::sexp::accessors::SET_VECTOR_ELT(outer, 0, inner);
            crate::sexp::envir::defineVar(sym, outer, crate::sexp::globals::R_GlobalEnv());
        }
        crate::sexp::gengc::full_gc();
        unsafe {
            assert_eq!(ALTREP(outer), 0);
            assert_eq!(ALTREP(inner), 0);
            assert_eq!(crate::sexp::accessors::VECTOR_ELT(outer, 0), inner);
            assert_eq!(
                crate::sexp::envir::R_findVarInFrame(crate::sexp::globals::R_GlobalEnv(), sym),
                outer
            );
        }
    }
}
