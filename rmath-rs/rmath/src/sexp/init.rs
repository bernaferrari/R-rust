//! R interpreter initialization.
//!
//! Initializes the active session's base bindings and common symbols. The
//! environment chain itself is owned by `RInstance`; there is intentionally no
//! process-global fallback interpreter.

use super::ffi::SEXP;
use super::instance::{
    RInstance, current_instance_ptr, replace_current_instance, with_required_current_instance,
};
use super::symbol::Rf_install_in;
use std::ffi::CString;

struct ScopedCurrentInstance {
    previous: Option<*mut RInstance>,
}

impl ScopedCurrentInstance {
    unsafe fn install(instance: *mut RInstance) -> Self {
        let previous = unsafe { replace_current_instance(Some(instance)) };
        Self { previous }
    }
}

impl Drop for ScopedCurrentInstance {
    fn drop(&mut self) {
        unsafe {
            replace_current_instance(self.previous);
        }
    }
}

pub fn is_initialized() -> bool {
    with_required_current_instance(is_initialized_in)
}

pub(crate) fn is_initialized_in(inst: &mut RInstance) -> bool {
    inst.initialized
}

pub unsafe fn initialize_r() {
    let instance = current_instance_ptr()
        .expect("mutable R runtime state requires an active RInstance for initialize_r");
    unsafe {
        initialize_r_in(&mut *instance);
    }
}

pub(crate) unsafe fn initialize_r_in(inst: &mut RInstance) {
    unsafe {
        super::context::install_r_panic_hook();
        if !inst.initialized {
            let base_env = inst.base_env;
            initialize_base_bindings_in(inst, base_env);
            inst.initialized = true;
        }
    }
}

/// Install the core bindings needed by a base environment.
///
/// This is used both by the legacy process-global initializer and by
/// per-session `RInstance` construction. It intentionally does not mutate the
/// process-global environment pointers.
pub unsafe fn initialize_base_bindings(base_env: SEXP) {
    let instance = current_instance_ptr().expect(
        "mutable R runtime state requires an active RInstance for initialize_base_bindings",
    );
    unsafe {
        initialize_base_bindings_in(&mut *instance, base_env);
    }
}

pub(crate) unsafe fn initialize_base_bindings_in(inst: &mut RInstance, base_env: SEXP) {
    unsafe {
        let _scope = ScopedCurrentInstance::install(inst as *mut RInstance);

        pre_intern_symbols_in(inst);
        crate::eval::jit::R_init_jit_enabled_in(inst);

        crate::eval::arithmetic::register_arithmetic_builtins(base_env);
        crate::eval::arithmetic::register_special_forms(base_env);
        crate::mainutils::essentials::register_essentials_builtins(base_env);
        crate::mainutils::rng_dispatch::register_rng_builtins(base_env);
    }
}

unsafe fn pre_intern_symbols() {
    with_required_current_instance(|inst| unsafe { pre_intern_symbols_in(inst) });
}

unsafe fn pre_intern_symbols_in(inst: &mut RInstance) {
    unsafe {
        let symbols = [
            "if",
            "else",
            "while",
            "for",
            "repeat",
            "break",
            "next",
            "function",
            "return",
            "invisible",
            "stop",
            "warning",
            "TRUE",
            "FALSE",
            "NULL",
            "NA",
            "Inf",
            "NaN",
            "library",
            "require",
            "data",
            "detach",
            "search",
            "source",
            "+",
            "-",
            "*",
            "/",
            "^",
            "%%",
            "%/%",
            "<",
            ">",
            "<=",
            ">=",
            "==",
            "!=",
            "!",
            "&",
            "&&",
            "|",
            "||",
            "<-",
            "<<-",
            "=",
            "->",
            "->>",
            "{",
            "(",
            "[",
            "[[",
            "$",
            "@",
            "::",
            ":::",
            "~",
            ":",
            "c",
            "list",
            "length",
            "names",
            "print",
            "cat",
            "paste",
            "paste0",
            "as.integer",
            "as.double",
            "as.character",
            "as.logical",
            "is.integer",
            "is.double",
            "is.character",
            "is.logical",
            "is.null",
            "is.na",
            "is.vector",
            "is.list",
            "sum",
            "mean",
            "min",
            "max",
            "range",
            "which",
            "which.min",
            "which.max",
            "seq",
            "seq_len",
            "seq_along",
            "rep",
            "matrix",
            "array",
            "dim",
            "nrow",
            "ncol",
            "apply",
            "sapply",
            "lapply",
            "vapply",
            "mapply",
            "t",
            "cbind",
            "rbind",
            "...",
            "..1",
            "..2",
            "..3",
            "..4",
            "..5",
            "missing",
            "on.exit",
            "sys.call",
            "match.arg",
        ];

        for name in &symbols {
            let c_name = CString::new(*name).expect("static R symbol name has no interior NUL");
            Rf_install_in(inst, c_name.as_ptr());
        }
    }
}

pub unsafe fn shutdown_r() {
    with_required_current_instance(shutdown_r_in);
}

pub(crate) fn shutdown_r_in(inst: &mut RInstance) {
    inst.initialized = false;
}

#[cfg(test)]
mod tests {
    use super::super::ffi::SEXPTYPE;
    use super::super::globals::{
        R_BaseEnv, R_BaseEnv_in, R_EmptyEnv, R_EmptyEnv_in, R_GlobalEnv, R_GlobalEnv_in,
    };
    use super::super::instance::RInstance;
    use super::super::symbol::Rf_install;
    use super::*;

    #[test]
    fn test_initialize_sets_environments() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            initialize_r();

            let global = R_GlobalEnv();
            let base = R_BaseEnv();
            let empty = R_EmptyEnv();

            assert!(!global.is_null());
            assert!(!base.is_null());
            assert!(!empty.is_null());

            assert_eq!((*global).sxpinfo.type_of(), SEXPTYPE::ENVSXP);
            assert_eq!((*base).sxpinfo.type_of(), SEXPTYPE::ENVSXP);
            assert_eq!((*empty).sxpinfo.type_of(), SEXPTYPE::ENVSXP);

            assert!(is_initialized());

            shutdown_r();
        }
    }

    #[test]
    fn test_initialize_base_bindings_use_canonical_primitive_identity() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            initialize_r();

            let base = R_BaseEnv();
            let plus = Rf_install(c"+".as_ptr());
            let if_sym = Rf_install(c"if".as_ptr());
            let log = Rf_install(c"log".as_ptr());

            let plus_val = crate::sexp::envir::R_findVarInFrame(base, plus);
            let if_val = crate::sexp::envir::R_findVarInFrame(base, if_sym);
            let log_val = crate::sexp::envir::R_findVarInFrame(base, log);

            assert_eq!(
                crate::eval::primitive::PrimitiveDescriptor::from_raw(plus_val)
                    .expect("+ primitive descriptor")
                    .name,
                "+"
            );
            assert_eq!(
                crate::eval::primitive::PrimitiveDescriptor::from_raw(if_val)
                    .expect("if primitive descriptor")
                    .name,
                "if"
            );
            assert!(
                crate::eval::primitive::PrimitiveDescriptor::from_raw(log_val).is_none(),
                "direct log binding is an evaluator helper, not a canonical R primitive"
            );
            assert_eq!(crate::sexp::accessors::PRIMOFFSET(log_val), -1);

            shutdown_r();
        }
    }

    #[test]
    fn test_idempotent() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            initialize_r();
            let g1 = R_GlobalEnv();

            initialize_r();
            let g2 = R_GlobalEnv();

            assert_eq!(g1, g2);

            shutdown_r();
        }
    }

    #[test]
    fn test_shutdown_clears() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            initialize_r();
            assert!(is_initialized());

            shutdown_r();
            assert!(!is_initialized());
            assert!(!R_GlobalEnv().is_null());
        }
    }

    #[test]
    fn test_pre_interned_symbols() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            initialize_r();

            let plus = Rf_install(c"+".as_ptr());
            assert!(!plus.is_null());

            let plus2 = Rf_install(c"+".as_ptr());
            assert_eq!(plus, plus2);

            let if_sym = Rf_install(c"if".as_ptr());
            assert!(!if_sym.is_null());

            shutdown_r();
        }
    }

    #[test]
    fn test_environment_chain() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            use super::super::globals::R_NilValue;
            initialize_r();

            let global = R_GlobalEnv();
            let base = R_BaseEnv();
            let empty = R_EmptyEnv();

            assert_eq!((*global).data.envsxp.enclos, base);
            assert_eq!((*base).data.envsxp.enclos, empty);
            assert_eq!((*empty).data.envsxp.enclos, R_NilValue());

            shutdown_r();
        }
    }

    #[test]
    fn test_initialization_can_target_instance_explicitly() {
        let mut left = RInstance::new();
        let mut right = RInstance::new();

        shutdown_r_in(&mut left);
        shutdown_r_in(&mut right);
        assert!(!is_initialized_in(&mut left));
        assert!(!is_initialized_in(&mut right));

        unsafe {
            initialize_r_in(&mut left);
        }

        assert!(is_initialized_in(&mut left));
        assert!(!is_initialized_in(&mut right));
        assert!(!R_GlobalEnv_in(&mut left).is_null());
        assert!(!R_BaseEnv_in(&mut left).is_null());
        assert!(!R_EmptyEnv_in(&mut left).is_null());
        assert!(!R_GlobalEnv_in(&mut right).is_null());

        let plus = unsafe { Rf_install_in(&mut left, c"+".as_ptr()) };
        let left_plus = unsafe {
            let _scope = ScopedCurrentInstance::install(&mut left as *mut RInstance);
            crate::sexp::envir::R_findVarInFrame(left.base_env, plus)
        };
        assert!(
            unsafe { crate::eval::primitive::PrimitiveDescriptor::from_raw(left_plus) }
                .is_some_and(|descriptor| descriptor.name == "+")
        );
        assert!(!is_initialized_in(&mut right));
    }
}
