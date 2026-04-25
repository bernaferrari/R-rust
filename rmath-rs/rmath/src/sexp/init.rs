//! R interpreter initialization.
//!
//! Sets up the global environment chain (empty -> base -> global),
//! pre-interns common symbols, and configures the interpreter for use.
//!
//! # Example
//!
//! ```rust,ignore
//! use rmath::sexp::init::initialize_r;
//!
//! unsafe { initialize_r(); }
//! // Now R_GlobalEnv, R_BaseEnv, R_EmptyEnv are all set up.
//! ```

use std::ffi::CString;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use super::globals::{set_R_BaseEnv, set_R_EmptyEnv, set_R_GlobalEnv};
use super::instance::with_current_instance;
use super::memory_ext::NewPersistentEnvironment;
use super::symbol::Rf_install;

static R_INITIALIZED: AtomicBool = AtomicBool::new(false);

static INIT_LOCK: Mutex<()> = Mutex::new(());

pub fn is_initialized() -> bool {
    with_current_instance(|inst| inst.initialized)
        .unwrap_or_else(|| R_INITIALIZED.load(Ordering::Acquire))
}

pub unsafe fn initialize_r() {
    unsafe {
        super::context::install_r_panic_hook();

        if let Some(()) = with_current_instance(|inst| {
            if !inst.initialized {
                initialize_base_bindings(inst.base_env);
                inst.initialized = true;
            }
        }) {
            return;
        }

        if R_INITIALIZED.load(Ordering::Acquire) {
            return;
        }

        let _guard = INIT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Double-check: another thread may have initialized while we waited for the lock.
        if R_INITIALIZED.load(Ordering::Acquire) {
            return;
        }

        let nil = super::globals::R_NilValue();

        let empty_env = NewPersistentEnvironment(nil, nil, nil);
        set_R_EmptyEnv(empty_env);

        let base_env = NewPersistentEnvironment(nil, empty_env, nil);
        set_R_BaseEnv(base_env);

        let global_env = NewPersistentEnvironment(nil, base_env, nil);
        set_R_GlobalEnv(global_env);

        initialize_base_bindings(base_env);

        R_INITIALIZED.store(true, Ordering::Release);
    }
}

/// Install the core bindings needed by a base environment.
///
/// This is used both by the legacy process-global initializer and by
/// per-session `RInstance` construction. It intentionally does not mutate the
/// process-global environment pointers.
pub unsafe fn initialize_base_bindings(base_env: super::ffi::SEXP) {
    unsafe {
        pre_intern_symbols();

        crate::eval::arithmetic::register_arithmetic_builtins(base_env);
        crate::eval::arithmetic::register_special_forms(base_env);
        crate::mainutils::essentials::register_essentials_builtins(base_env);
        crate::mainutils::rng_dispatch::register_rng_builtins(base_env);
    }
}

unsafe fn pre_intern_symbols() {
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
            Rf_install(c_name.as_ptr());
        }
    }
}

pub unsafe fn shutdown_r() {
    unsafe {
        if let Some(()) = with_current_instance(|inst| {
            inst.initialized = false;
        }) {
            return;
        }

        if !R_INITIALIZED.load(Ordering::Acquire) {
            return;
        }

        let _guard = INIT_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        set_R_GlobalEnv(std::ptr::null_mut());
        set_R_BaseEnv(std::ptr::null_mut());
        set_R_EmptyEnv(std::ptr::null_mut());

        R_INITIALIZED.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::super::ffi::SEXPTYPE;
    use super::super::globals::{R_BaseEnv, R_EmptyEnv, R_GlobalEnv};
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
}
