//! Essential R built-in functions — c(), seq(), rep(), paste(), cat(), typeof(), is.na(), names().
//!
//! These are the most fundamental R functions that every R program uses.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{CStr, CString};
use std::os::raw::c_int;
use std::path::{Path, PathBuf};

#[allow(unused_imports)]
use crate::sexp::accessors::{
    ATTRIB, CADR, CAR, CDR, CHAR, COMPLEX, FORMALS, FRAME, HASHTAB, INTEGER, INTEGER_ELT, LENGTH,
    LOGICAL, LOGICAL_ELT, PRINTNAME, RAW, REAL, REAL_ELT, SET_ENCLOS, SET_OBJECT, SET_STRING_ELT,
    SET_VECTOR_ELT, SETCAR, SETCDR, SETTAG, STRING_ELT, TAG, TYPEOF, VECTOR_ELT, XLENGTH,
};
#[allow(unused_imports)]
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3, Rf_cons, Rf_lang2,
    Rf_lang3, Rf_mkChar, Rf_mkString,
};

use crate::sexp::context::RError;
use crate::sexp::ffi::{
    ISNAN, NA_INTEGER, NA_LOGICAL, NA_REAL, R_NA_BIT_PATTERN, R_xlen_t, SEXP, SEXPTYPE, TRUE,
};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

use super::*;

mod namespace_conditions;
pub(crate) use namespace_parse::s3_method_function_names;

#[cfg(test)]
mod native_s3_binding_tests;

thread_local! {
    static CACHING_ATTACHED_S4: Cell<bool> = const { Cell::new(false) };
}

#[cfg(test)]
mod methods_startup_tests {
    use super::*;

    #[test]
    fn stats_only_namespace_startup_registers_method_table_and_survives_gc() {
        let mut session = crate::sexp::session::RSession::new_without_default_packages();
        // This is an actual installed-package startup test. The base-only
        // collector fixture intentionally has no host paths, so opt in to
        // the same library discovery policy as an ordinary desktop session.
        session.set_library_paths(
            crate::mainutils::paths::RuntimePathPolicy::from_env()
                .library_paths()
                .to_vec(),
        );
        session.with_active(|| unsafe {
            let factory = session.owner_token().unwrap().node_factory();
            let namespace = factory
                .wrap(
                    load_package_namespace_by_name("stats")
                        .expect("stats namespace must load independently of default attachments"),
                )
                .unwrap();
            assert_eq!(namespace.typeof_(), SEXPTYPE::ENVSXP);
            let table = factory
                .wrap(crate::sexp::envir::R_findVarInFrame(
                    namespace.as_raw(),
                    Rf_install(c".__S3MethodsTable__.".as_ptr()),
                ))
                .unwrap();
            assert_eq!(table.typeof_(), SEXPTYPE::ENVSXP);
            let method_symbol = factory.wrap(Rf_install(c"predict.lm".as_ptr())).unwrap();
            let method =
                crate::sexp::envir::find_var_in_frame_result(table.clone(), method_symbol.clone())
                    .unwrap()
                    .expect("stats must register predict.lm");
            // Namespace startup registers GNU lazy promises; forcing is a
            // dispatch obligation, not a prerequisite for registration.
            assert!(is_function_value(method.as_raw()) || method.typeof_() == SEXPTYPE::PROMSXP);
            let callable = factory
                .wrap(crate::eval::eval::Rf_eval(
                    method.as_raw(),
                    namespace.as_raw(),
                ))
                .unwrap();
            assert!(is_function_value(callable.as_raw()));
            session.owner_token().unwrap().full_gc().unwrap();
            assert!(is_function_value(callable.as_raw()));
            assert_eq!(
                crate::sexp::envir::find_var_in_frame_result(table, method_symbol)
                    .unwrap()
                    .unwrap(),
                method
            );
            assert_eq!(
                factory
                    .wrap(cached_namespace_by_name("stats").unwrap())
                    .unwrap(),
                namespace
            );
        });
    }

    #[test]
    fn methods_source_guard_prevents_recompilation_at_actual_closure_entry() {
        use std::{cell::Cell, rc::Rc};
        let mut session = crate::sexp::session::RSession::new_without_default_packages();
        let (allocation, _function_root) = {
            let (result, _, _) = session.eval_script_with_output_capture("function() { 1L }");
            let function = result.unwrap();
            let allocation = function.allocation().unwrap().clone();
            let root = allocation.root_lease().unwrap();
            (allocation, root)
        };
        session.with_active(|| unsafe {
            crate::eval::jit::set_R_jit_enabled(3);
            let owner = session.owner_token().unwrap();
            let function = owner
                .node_factory()
                .wrap(
                    allocation
                        .heap_identity()
                        .node_projection(&allocation)
                        .unwrap(),
                )
                .unwrap();
            crate::eval::jit::set_R_min_jit_score_in(owner.as_ptr(), 0);
            assert!(crate::eval::jit::R_cmpfun(function.as_raw()));
            let original = function.try_body().unwrap();
            assert!(original.is_bytecode());
            let seen = Rc::new(Cell::new(0));
            let observed = seen.clone();
            let inspected = function.allocation().unwrap().clone();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                observed.set(observed.get() + 1);
                let heap = inspected.heap_identity();
                let body = heap.node_snapshot(&inspected).unwrap().data.closure().body;
                let Some(crate::sexp::heap::ResolvedLink::Node { allocation, .. }) =
                    heap.resolve_link(body)
                else {
                    panic!("metadata source body must retain its exact language allocation");
                };
                assert_eq!(
                    heap.node_snapshot(&allocation).unwrap().sxpinfo.type_of(),
                    SEXPTYPE::LANGSXP
                );
                assert_eq!(crate::eval::jit::get_R_jit_enabled(), 0);
            }));
            {
                let _source = methods_metadata_source_body(function.as_raw()).unwrap();
                (*owner.as_ptr()).memory_state.gc_force_gap = 1;
                (*owner.as_ptr()).memory_state.gc_force_wait = 1;
                let mut call = crate::sexp::object::PairlistBuilder::new_in(owner);
                call.push(function.clone(), None).unwrap();
                let call = call.finish_as_type(SEXPTYPE::LANGSXP).unwrap();
                let raw =
                    crate::eval::eval::Rf_eval(call.as_raw(), crate::sexp::globals::R_GlobalEnv());
                let result = owner.node_factory().wrap(raw).unwrap();
                assert_eq!(result.try_integer_elt(0).unwrap(), 1);
                assert_eq!(function.try_body().unwrap().typeof_(), SEXPTYPE::LANGSXP);
                assert!(seen.get() > 0);
            }
            assert_eq!(function.try_body().unwrap(), original);
            assert_eq!(crate::eval::jit::get_R_jit_enabled(), 3);
            // The same guard restores state during unwind and preserves a
            // callback's explicit change rather than overwriting it.
            let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _source = methods_metadata_source_body(function.as_raw()).unwrap();
                crate::sexp::context::r_error("source guard unwind");
            }));
            assert!(failed.is_err());
            assert_eq!(function.try_body().unwrap(), original);
            assert_eq!(crate::eval::jit::get_R_jit_enabled(), 3);
            {
                let _source = methods_metadata_source_body(function.as_raw()).unwrap();
                crate::eval::jit::set_R_jit_enabled(2);
            }
            assert_eq!(function.try_body().unwrap(), original);
            assert_eq!(crate::eval::jit::get_R_jit_enabled(), 2);
        });
    }

    #[test]
    fn default_methods_startup_populates_metadata_and_restores_compiled_body() {
        let mut session = crate::sexp::session::RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            "c(.isMethodsDispatchOn(),
               !is.null(methods::getClassDef('envRefClass')),
               !is.null(methods:::.getClassesFromCache('envRefClass')),
               is.environment(methods::getClassDef('envRefClass')@refMethods))",
        );
        let metadata = result.unwrap_or_else(|error| {
            panic!("default methods metadata failed: {error:?}; output: {output:?}")
        });
        assert_eq!(metadata.len(), 4);
        for index in 0..4 {
            assert_eq!(metadata.try_logical_elt(index).unwrap(), TRUE);
        }
        drop(metadata);
        session.with_active(|| unsafe {
            let ns = cached_namespace_by_name("methods").expect("default methods namespace loaded");
            let raw =
                crate::sexp::envir::R_findVarInFrame(ns, Rf_install(c"cacheMetaData".as_ptr()));
            let raw = if TYPEOF(raw) == SEXPTYPE::PROMSXP {
                crate::sexp::envir::forcePromise(raw)
            } else {
                raw
            };
            let factory =
                crate::sexp::object::SessionNodeFactory::new(session.owner_token().unwrap());
            let function = factory.wrap(raw).unwrap();
            // Successful default startup must have restored the installed
            // bytecode, rather than permanently replacing the package image.
            let original = function.try_body().unwrap();
            assert!(original.is_bytecode());
            let original_link = factory.link(&original).unwrap();
            let before = crate::sexp::protect::R_ProtectCount();
            let jit_before = crate::eval::jit::get_R_jit_enabled();
            let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _source_body = methods_metadata_source_body(function.as_raw())
                    .expect("compiled cacheMetaData has its stored language expression");
                assert_eq!(function.try_body().unwrap().typeof_(), SEXPTYPE::LANGSXP);
                assert_eq!(crate::eval::jit::get_R_jit_enabled(), 0);
                assert_eq!(
                    crate::eval::jit::R_CheckJIT(function.as_raw()),
                    crate::sexp::ffi::FALSE
                );
                crate::sexp::gengc::full_gc();
                assert_eq!(crate::sexp::protect::R_ProtectCount(), before);
                crate::sexp::context::r_error("metadata evaluation unwind regression");
            }))
            .expect_err("injected R error must unwind through body restoration");
            assert!(
                failure
                    .downcast_ref::<crate::sexp::context::RError>()
                    .is_some()
            );
            let restored = function.try_body().unwrap();
            assert_eq!(restored, original);
            assert_eq!(factory.link(&restored).unwrap(), original_link);
            assert_eq!(crate::eval::jit::get_R_jit_enabled(), jit_before);
            assert_eq!(crate::sexp::protect::R_ProtectCount(), before);
            crate::sexp::gengc::full_gc();
            assert!(function.try_body().unwrap().is_bytecode());
        });
    }
}

#[cfg(test)]
#[path = "shared/implicit_generics_tests.rs"]
mod implicit_generics_tests;

#[cfg(test)]
#[path = "shared/methods_cache_tests.rs"]
mod methods_cache_tests;

mod access_dollar;
mod installed;
mod lazyload;
mod namespace_core;
mod namespace_load;
mod namespace_parse;
mod namespace_populate;
mod package_paths;
mod runtime_misc;
mod string_vectors;

#[allow(unused_imports)]
pub use self::access_dollar::*;

#[allow(unused_imports)]
pub use self::string_vectors::*;

#[allow(unused_imports)]
pub use self::package_paths::*;

#[allow(unused_imports)]
pub use self::installed::*;

#[allow(unused_imports)]
pub use self::namespace_load::*;

#[allow(unused_imports)]
pub use self::namespace_core::*;

#[allow(unused_imports)]
pub use self::lazyload::*;

#[allow(unused_imports)]
pub use self::namespace_populate::*;

#[allow(unused_imports)]
pub use self::namespace_parse::*;

#[allow(unused_imports)]
pub use self::runtime_misc::*;
