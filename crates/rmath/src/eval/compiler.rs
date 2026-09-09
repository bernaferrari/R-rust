//! Small built-in surface for the portable `compiler` namespace.
//!
//! The full GNU R compiler package is not sourced by this runtime.  This
//! namespace therefore exposes only `cmpfun`, backed by the private compiler
//! and with strict failure for syntax/options outside the supported subset.

use std::ffi::CStr;

use crate::eval::jit::{compiler_cmpfun, compiler_enable_jit};
use crate::sexp::accessors::{CAR, CDR, CHAR, PRINTNAME, TAG, TYPEOF};
use crate::sexp::constructors::Rf_ScalarInteger;
use crate::sexp::context::RError;
use crate::sexp::envir::defineVar;
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals::{R_BaseEnv, R_NilValue};
use crate::sexp::instance::with_required_current_instance;
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

/// Public names implemented by the portable compiler namespace.
pub(crate) const EXPORTS: &[&str] = &["cmpfun", "enableJIT"];

fn compiler_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(RError {
        message: message.into(),
    });
}

unsafe fn tag_name(cell: SEXP) -> Option<String> {
    unsafe {
        let tag = TAG(cell);
        if tag.is_null() || tag == R_NilValue() || TYPEOF(tag) != SEXPTYPE::SYMSXP {
            return None;
        }
        let printname = PRINTNAME(tag);
        if printname.is_null() {
            return None;
        }
        let chars = CHAR(printname);
        if chars.is_null() {
            return None;
        }
        Some(CStr::from_ptr(chars).to_string_lossy().into_owned())
    }
}

/// `compiler::cmpfun(f, options = NULL)` for the supported portable subset.
///
/// Arguments are already evaluated by the builtin dispatcher. `options` is
/// accepted only when omitted or explicitly `NULL`; silently ignoring a GNU R
/// compiler option would make the result misleading.
pub unsafe fn do_cmpfun(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Named formals are matched before unnamed arguments, independent of
        // their order in the call. Both names have distinct initial letters,
        // so nonempty prefixes are unambiguous GNU-style partial matches.
        let mut matched = [None, None];
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if let Some(name) = tag_name(current) {
                let index = if !name.is_empty() && "f".starts_with(&name) {
                    0
                } else if !name.is_empty() && "options".starts_with(&name) {
                    1
                } else {
                    compiler_error(format!("unused argument ({name} = ... )"));
                };
                if matched[index].replace(CAR(current)).is_some() {
                    compiler_error(format!(
                        "formal argument '{}' matched by multiple actual arguments",
                        ["f", "options"][index]
                    ));
                }
            }
            current = CDR(current);
        }
        current = args;
        while !current.is_null() && current != R_NilValue() {
            if tag_name(current).is_none() {
                let Some(slot) = matched.iter_mut().find(|slot| slot.is_none()) else {
                    compiler_error("unused argument (...)");
                };
                *slot = Some(CAR(current));
            }
            current = CDR(current);
        }
        let [fun, options] = matched;

        let Some(fun) = fun else {
            compiler_error("argument 'f' is missing, with no default");
        };
        if fun.is_null() || fun == R_NilValue() {
            compiler_error("cannot compile a non-function");
        }
        if let Some(options) = options
            && !options.is_null()
            && options != R_NilValue()
        {
            compiler_error("portable compiler does not support compiler options");
        }

        match compiler_cmpfun(fun) {
            Ok(compiled) => compiled,
            Err(message) => compiler_error(message),
        }
    }
}

/// `compiler::enableJIT(level)`, backed by the session-local JIT state used
/// on every closure invocation.
pub unsafe fn do_enable_jit(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        if args.is_null()
            || args == R_NilValue()
            || CAR(args) == crate::sexp::globals::R_MissingArg()
        {
            compiler_error("argument 'level' is missing, with no default");
        }
        let rest = CDR(args);
        if !rest.is_null() && rest != R_NilValue() {
            compiler_error("unused argument (...)");
        }
        if let Some(name) = tag_name(args)
            && (name.is_empty() || !"level".starts_with(&name))
        {
            compiler_error(format!("unused argument ({name} = ... )"));
        }
        Rf_ScalarInteger(compiler_enable_jit(CAR(args)))
    }
}

unsafe fn new_compiler_environment() -> SEXP {
    unsafe {
        let env = crate::sexp::memory_ext::NewEnvironment(R_NilValue(), R_BaseEnv(), R_NilValue());
        let _guard = protect(env);
        crate::mainutils::essentials::define_package_metadata("compiler", env);
        for name in EXPORTS {
            let symbol = Rf_install(std::ffi::CString::new(*name).unwrap().as_ptr());
            let value = crate::eval::primitive::make_primitive_binding(name, SEXPTYPE::BUILTINSXP);
            let _value_guard = protect(value);
            defineVar(symbol, value, env);
        }
        env
    }
}

/// Return the per-session compiler namespace, creating it on first lookup.
pub(crate) unsafe fn namespace() -> SEXP {
    unsafe {
        if let Some(env) = with_required_current_instance(|inst| {
            (*inst)
                .package_namespace_cache
                .get("compiler")
                .map(|(_, env)| *env)
        }) {
            return env;
        }
        let env = new_compiler_environment();
        with_required_current_instance(|inst| {
            (*inst).package_namespace_cache.insert(
                "compiler".into(),
                (std::path::PathBuf::from("<builtin:compiler>"), env),
            );
        });
        env
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::session::RSession;

    #[test]
    fn compiler_namespace_exposes_supported_compiler_controls() {
        let _session = RSession::new();
        let namespace = unsafe { namespace() };
        assert_ne!(namespace, unsafe { R_NilValue() });
        assert_eq!(EXPORTS, &["cmpfun", "enableJIT"]);
    }

    #[test]
    fn cmpfun_options_rejects_non_null_values() {
        let session = RSession::new();
        session.with_active(|| unsafe {
            let formals = crate::sexp::constructors::Rf_allocList(0);
            let body = crate::sexp::constructors::Rf_ScalarInteger(1);
            let fun = crate::mainutils::dstruct::mkCLOSXP(
                formals,
                body,
                crate::sexp::globals::R_GlobalEnv(),
            );
            let args = crate::sexp::constructors::Rf_cons(
                fun,
                crate::sexp::constructors::Rf_cons(
                    crate::sexp::constructors::Rf_ScalarInteger(1),
                    R_NilValue(),
                ),
            );
            crate::sexp::accessors::SETTAG(
                crate::sexp::accessors::CDR(args),
                Rf_install(c"options".as_ptr()),
            );
            let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                do_cmpfun(R_NilValue(), R_NilValue(), args, R_NilValue());
            }))
            .expect_err("non-null options should be rejected");
            assert!(
                panic
                    .downcast_ref::<RError>()
                    .is_some_and(|error| error.message.contains("compiler options"))
            );
        });
    }
}
