//! R interpreter initialization.
//!
//! Initializes the active session's base bindings and common symbols. The
//! environment chain itself is owned by `RInstance`; there is intentionally no
//! process-global fallback interpreter.

use super::accessors::{CDR, SET_SYMVALUE, SETCAR, SETTAG, TYPEOF};

use super::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_allocList, Rf_lang2, Rf_lang3, Rf_mkString,
};
use super::envir::{R_findVarInFrame, defineVar};
use super::ffi::{FALSE, SEXP, SEXPTYPE, TRUE};
use super::globals::{R_EmptyEnv, R_MissingArg, R_NilValue, R_UnboundValue};
use super::instance::{RInstance, current_instance_ptr, with_required_current_instance};
use super::symbol::Rf_install_in;
use std::ffi::CString;

/// Acquire the sole original interpreter before initialization can execute R.
/// No borrow of an instance field survives callback-capable bootstrap work.
unsafe fn initialization_owner_pin(instance: *mut RInstance) -> super::owner::OwnerPin {
    let owner = unsafe { (*instance).runtime_owner.clone() }.unwrap_or_else(|| {
        super::context::r_error("initialization requires a managed runtime owner")
    });
    let pin = owner
        .pin()
        .unwrap_or_else(|error| super::context::r_error(error.to_string()));
    if pin.as_ptr() != instance {
        super::context::r_error("initialization requires the original runtime allocation");
    }
    pin
}

fn require_initialization_live(pin: &super::owner::OwnerPin) {
    pin.require_live()
        .unwrap_or_else(|error| super::context::r_error(error.to_string()));
}

pub fn is_initialized() -> bool {
    with_required_current_instance(is_initialized_in)
}

pub(crate) fn is_initialized_in(inst: *mut RInstance) -> bool {
    // P2: single-field read; no ambient write intervenes.
    unsafe { (*inst).initialized }
}

pub unsafe fn initialize_r() {
    let instance = current_instance_ptr()
        .expect("mutable R runtime state requires an active RInstance for initialize_r");
    unsafe {
        initialize_r_in(instance);
    }
}

pub(crate) unsafe fn initialize_r_in(inst: *mut RInstance) {
    unsafe {
        let pin = initialization_owner_pin(inst);
        super::context::install_r_panic_hook();
        // P1: raw place accesses — initialize_base_bindings_in reenters the
        // interpreter (symbol interning, protect pushes, builtin tables).
        if !(*inst).initialized {
            let base_env = (*inst).base_env;
            initialize_base_bindings_in(inst, base_env);
            require_initialization_live(&pin);
            (*inst).initialized = true;
        }
    }
}

/// Install the core bindings needed by a base environment.
///
/// Bootstrap runs on the original managed session allocation. Scoped
/// activation owns both the target and the previous interpreter through any
/// callback, and restores only a previous owner that remains available.
pub unsafe fn initialize_base_bindings(base_env: SEXP) {
    let instance = current_instance_ptr().expect(
        "mutable R runtime state requires an active RInstance for initialize_base_bindings",
    );
    unsafe {
        initialize_base_bindings_in(instance, base_env);
    }
}

pub(crate) unsafe fn initialize_base_bindings_in(inst: *mut RInstance, base_env: SEXP) {
    unsafe {
        let pin = initialization_owner_pin(inst);
        let owner = super::owner::OwnerToken::from_raw(inst);
        let environment = owner
            .sexp(base_env)
            .and_then(|value| value.into_owned())
            .unwrap_or_else(|error| {
                super::context::r_error(format!("invalid initialization environment: {error}"))
            });
        if environment.typeof_() != SEXPTYPE::ENVSXP {
            super::context::r_error("initialization requires a base environment");
        }
        let base_env = environment.as_raw();
        super::session::with_instance_active(inst, || {
            pre_intern_symbols_in(inst);
            require_initialization_live(&pin);
            crate::eval::jit::R_init_jit_enabled_in(inst);
            require_initialization_live(&pin);
            crate::eval::arithmetic::register_special_forms(base_env);
            require_initialization_live(&pin);
            crate::mainutils::essentials::register_essentials_builtins(base_env);
            require_initialization_live(&pin);
            initialize_special_environment_bindings(base_env);
            require_initialization_live(&pin);
            crate::mainutils::machine::Init_R_Machine(base_env);
            require_initialization_live(&pin);
            crate::mainutils::options::InitOptions();
            require_initialization_live(&pin);
            initialize_base_functions(base_env);
            require_initialization_live(&pin);
            initialize_primitive_metadata_in(base_env);
            require_initialization_live(&pin);
            crate::library::stats::init::R_init_stats(std::ptr::null_mut());
            require_initialization_live(&pin);
        });
    }
}

/// Install base functions normally loaded from `src/library/base/R`.
///
/// The port does not yet source the complete base package during startup, so
/// small language-level definitions needed by base itself must be installed
/// explicitly. Keep these as ordinary closures rather than evaluator
/// shortcuts so argument promises retain GNU R's lazy semantics.
unsafe fn initialize_base_functions(base_env: SEXP) {
    unsafe {
        bindings_core::install(base_env);
        bindings_containers::install(base_env);
        bindings_format::install(base_env);
        bindings_ops::install(base_env);
        bindings_misc::install(base_env);
        bindings_datetime::install(base_env);
        // GNU base/R/zzz.R: these names share one primitive, so S4 methods
        // set on `as.numeric` / `is.symbol` dispatch from the alias too.
        // `.Primitive("as.numeric")` stays the distinct FunTab entry.
        alias_visible_primitive(base_env, "as.numeric", "as.double");
        alias_visible_primitive(base_env, "is.name", "is.symbol");
        retarget_base_closure_envs(base_env);
        // Upstream base-package sources: runs after `.BaseNamespaceEnv`
        // points at the shared namespace frame so staged closures resolve
        // free variables the way GNU base closures do.
        super::upstream_base::load_upstream_base_sources(base_env);
    }
}

unsafe fn alias_visible_primitive(base_env: SEXP, alias: &str, target: &str) {
    unsafe {
        let value = R_findVarInFrame(base_env, Rf_install_in_current(target));
        if value.is_null() || value == R_UnboundValue() || value == R_NilValue() {
            return;
        }
        let alias_sym = Rf_install_in_current(alias);
        defineVar(alias_sym, value, base_env);
        SET_SYMVALUE(alias_sym, value);
    }
}

unsafe fn retarget_base_closure_envs(base_env: SEXP) {
    unsafe {
        let global = super::globals::R_GlobalEnv();
        let namespace = crate::sexp::memory_ext::NewEnvironment(
            super::accessors::FRAME(base_env),
            global,
            super::globals::R_NilValue(),
        );
        let _guard = super::protect::protect(namespace);
        let mut cell = super::accessors::FRAME(base_env);
        while !cell.is_null() && cell != super::globals::R_NilValue() {
            let value = super::accessors::CAR(cell);
            if super::accessors::TYPEOF(value) == super::ffi::SEXPTYPE::CLOSXP {
                super::accessors::SET_CLOENV(value, namespace);
            }
            cell = super::accessors::CDR(cell);
        }
        // GNU InitGlobalEnv sets this symbol's value to R_BaseNamespace.
        // `isBaseFun` is `identical(environment(fun), .BaseNamespaceEnv)`, and
        // `getPackageName` returns "base" for that same pointer. The binding
        // already exists, so this updates the shared frame in place.
        let sym = Rf_install_in_current(".BaseNamespaceEnv");
        defineVar(sym, namespace, base_env);
        SET_SYMVALUE(sym, namespace);

        // GNU base/R/namespace.R gives the base namespace its table for
        // registered methods while base is still unsealed; after session
        // bootstrap locks base, nothing may add bindings to it. The define
        // mirrors into the shared base frame like every other base binding.
        crate::mainutils::essentials::ensure_s3_methods_table(namespace)
            .expect("base namespace requires an S3 methods table");
    }
}

unsafe fn eval_base_binding(base_env: SEXP, name: &str, source: &str) {
    unsafe {
        let parser_factory = crate::eval::parser::active_factory();
        let parsed = super::memory::with_arena(|arena| {
            crate::eval::parser::parse_expressions(source, arena, parser_factory.clone())
        });
        crate::eval::parser::flush_literal_warnings();
        let Ok(exprs) = parsed else {
            return;
        };
        if exprs.len() != 1 {
            return;
        }
        let value = crate::eval::eval::Rf_eval(exprs[0].clone().as_raw(), base_env);
        let _v = super::protect::protect(value);
        let symbol = Rf_install_in_current(name);
        defineVar(symbol, value, base_env);
        // GNU defineVar writes the closure into the symbol value slot.
        // Deparse uses SYMVALUE to distinguish primitives (PP_FUNCALL,
        // inlist++) from closures (plain call, no inlist++).
        SET_SYMVALUE(symbol, value);
    }
}

/// Bind `.GlobalEnv` inside the fresh base environment.
///
/// Deliberately takes no `&mut RInstance`: `defineVar` and the globals
/// accessors re-acquire the instance from the thread-local, and calling them
/// from a frame that holds an instance borrow would have that re-acquisition
/// pop a live protected borrow (aliasing UB under Stacked Borrows). The
/// current instance at this point is exactly the one being initialized.
unsafe fn initialize_special_environment_bindings(base_env: SEXP) {
    unsafe {
        defineVar(
            Rf_install_in_current(".GlobalEnv"),
            super::globals::R_GlobalEnv(),
            base_env,
        );
        // GNU startup.c: `.Library` is R_HOME/library. Upstream tests
        // (`classes-methods.R`, `eval-etc-2.R`) pass it as lib.loc.
        let library = with_required_current_instance(|inst| {
            if let Some(home) = std::env::var_os("R_HOME") {
                return std::path::PathBuf::from(home).join("library");
            }
            (*inst)
                .path_policy
                .library_paths()
                .iter()
                .find(|path| {
                    path.join("methods").join("DESCRIPTION").is_file()
                        || path.join("base").join("DESCRIPTION").is_file()
                })
                .cloned()
                .or_else(|| (*inst).path_policy.library_paths().first().cloned())
                .unwrap_or_else(|| std::path::PathBuf::from("/usr/lib/R/library"))
        });
        let library = CString::new(library.to_string_lossy().as_ref()).unwrap_or_default();
        defineVar(
            Rf_install_in_current(".Library"),
            Rf_mkString(library.as_ptr()),
            base_env,
        );
        // GNU puts an empty "Autoloads" environment on the search path
        // between .GlobalEnv and base. ls("Autoloads") resolves it by name.
        let global = super::globals::R_GlobalEnv();
        let autoloads = super::memory_ext::NewEnvironment(
            super::globals::R_NilValue(),
            super::accessors::ENCLOS(global),
            super::globals::R_NilValue(),
        );
        let _autoloads = super::protect::protect(autoloads);
        super::attrib_core::setAttrib(
            autoloads,
            Rf_install_in_current("name"),
            Rf_mkString(c"Autoloads".as_ptr()),
        );
        super::accessors::SET_ENCLOS(global, autoloads);
        defineVar(Rf_install_in_current(".AutoloadEnv"), autoloads, base_env);
        defineVar(
            Rf_install_in_current(".Autoloaded"),
            super::globals::R_NilValue(),
            autoloads,
        );
    }
}

#[derive(Clone, Copy)]
enum FormalDefault {
    Missing,
    Null,
    False,
    True,
    Int(i32),
    String(&'static str),
    ExpOne,
}

#[derive(Clone, Copy)]
struct FormalSpec {
    name: &'static str,
    default: FormalDefault,
}

#[derive(Clone, Copy)]
struct PrimitivePrototype {
    name: &'static str,
    formals: &'static [FormalSpec],
    generic: bool,
}

const fn arg(name: &'static str) -> FormalSpec {
    FormalSpec {
        name,
        default: FormalDefault::Missing,
    }
}

const fn arg_default(name: &'static str, default: FormalDefault) -> FormalSpec {
    FormalSpec { name, default }
}

const NO_ARGS: &[FormalSpec] = &[];
const DOTS: &[FormalSpec] = &[arg("...")];
const X: &[FormalSpec] = &[arg("x")];
const Z: &[FormalSpec] = &[arg("z")];
const E1_E2: &[FormalSpec] = &[arg("e1"), arg("e2")];
const X_Y: &[FormalSpec] = &[arg("x"), arg_default("y", FormalDefault::Null), arg("...")];
const SUMMARIES: &[FormalSpec] = &[arg("..."), arg_default("na.rm", FormalDefault::False)];

const NON_GENERIC_PROTOTYPES: &[PrimitivePrototype] = &[
    proto("::", &[arg("pkg"), arg("name")], false),
    proto(":::", &[arg("pkg"), arg("name")], false),
    proto("...length", NO_ARGS, false),
    proto("...names", NO_ARGS, false),
    // GNU base/R/rank.R and base/R/rep.R bind these as closures over
    // .Internal. They stay out of .ArgsEnv so primitives.R does not treat
    // the internal dispatch as an S3 generic of the visible function.
    proto("...elt", &[arg("n")], false),
    proto(
        ".C",
        &[
            arg(".NAME"),
            arg("..."),
            arg_default("NAOK", FormalDefault::False),
            arg_default("DUP", FormalDefault::True),
            arg("PACKAGE"),
            arg("ENCODING"),
        ],
        false,
    ),
    proto(
        ".Fortran",
        &[
            arg(".NAME"),
            arg("..."),
            arg_default("NAOK", FormalDefault::False),
            arg_default("DUP", FormalDefault::True),
            arg("PACKAGE"),
            arg("ENCODING"),
        ],
        false,
    ),
    proto(".Call", &[arg(".NAME"), arg("..."), arg("PACKAGE")], false),
    proto(
        ".Call.graphics",
        &[arg(".NAME"), arg("..."), arg("PACKAGE")],
        false,
    ),
    proto(
        ".External",
        &[arg(".NAME"), arg("..."), arg("PACKAGE")],
        false,
    ),
    proto(
        ".External2",
        &[arg(".NAME"), arg("..."), arg("PACKAGE")],
        false,
    ),
    proto(
        ".External.graphics",
        &[arg(".NAME"), arg("..."), arg("PACKAGE")],
        false,
    ),
    proto(".Internal", &[arg("call")], false),
    proto(".Primitive", &[arg("name")], false),
    proto(".class2", X, false),
    proto(
        ".isMethodsDispatchOn",
        &[arg_default("onOff", FormalDefault::Null)],
        false,
    ),
    proto(".primTrace", &[arg("obj")], false),
    proto(".primUntrace", &[arg("obj")], false),
    proto(".subset", &[arg("x"), arg("...")], false),
    proto(".subset2", &[arg("x"), arg("...")], false),
    proto("UseMethod", &[arg("generic"), arg("object")], false),
    proto(
        "attr",
        &[
            arg("x"),
            arg("which"),
            arg_default("exact", FormalDefault::False),
        ],
        false,
    ),
    proto("attr<-", &[arg("x"), arg("which"), arg("value")], false),
    proto("attributes", X, false),
    proto("attributes<-", &[arg("x"), arg("value")], false),
    proto("baseenv", NO_ARGS, false),
    proto(
        "browser",
        &[
            arg_default("text", FormalDefault::String("")),
            arg_default("condition", FormalDefault::Null),
            arg_default("expr", FormalDefault::True),
            arg_default("skipCalls", FormalDefault::Int(0)),
        ],
        false,
    ),
    proto("call", &[arg("name"), arg("...")], false),
    proto("class", X, false),
    proto("class<-", &[arg("x"), arg("value")], false),
    proto(".cache_class", &[arg("class"), arg("extends")], false),
    proto("declare", DOTS, false),
    proto("emptyenv", NO_ARGS, false),
    proto("enc2native", X, false),
    proto("enc2utf8", X, false),
    proto("environment<-", &[arg("fun"), arg("value")], false),
    proto("expression", DOTS, false),
    proto("forceAndCall", &[arg("n"), arg("FUN"), arg("...")], false),
    proto("gc.time", &[arg_default("on", FormalDefault::True)], false),
    proto("globalenv", NO_ARGS, false),
    proto("interactive", NO_ARGS, false),
    proto("invisible", &[arg_default("x", FormalDefault::Null)], false),
    proto("is.atomic", X, false),
    proto("is.call", X, false),
    proto("is.character", X, false),
    proto("is.complex", X, false),
    proto("is.double", X, false),
    proto("is.environment", X, false),
    proto("is.expression", X, false),
    proto("is.function", X, false),
    proto("is.integer", X, false),
    proto("is.language", X, false),
    proto("is.list", X, false),
    proto("is.logical", X, false),
    proto("is.name", X, false),
    proto("is.null", X, false),
    proto("is.object", X, false),
    proto("is.pairlist", X, false),
    proto("is.raw", X, false),
    proto("is.recursive", X, false),
    proto("is.single", X, false),
    proto("is.symbol", X, false),
    proto("isS4", &[arg("object")], false),
    proto("list", DOTS, false),
    proto(
        "lazyLoadDBfetch",
        &[arg("key"), arg("file"), arg("compressed"), arg("hook")],
        false,
    ),
    proto("missing", X, false),
    proto("nargs", NO_ARGS, false),
    proto(
        "nzchar",
        &[arg("x"), arg_default("keepNA", FormalDefault::False)],
        false,
    ),
    proto("oldClass", X, false),
    proto("oldClass<-", &[arg("x"), arg("value")], false),
    proto(
        "on.exit",
        &[
            arg_default("expr", FormalDefault::Null),
            arg_default("add", FormalDefault::False),
            arg_default("after", FormalDefault::True),
        ],
        false,
    ),
    proto("pos.to.env", X, false),
    proto("proc.time", NO_ARGS, false),
    proto("quote", &[arg("expr")], false),
    proto(
        "retracemem",
        &[arg("x"), arg_default("previous", FormalDefault::Null)],
        false,
    ),
    proto("seq_along", &[arg("along.with")], false),
    proto("seq_len", &[arg("length.out")], false),
    proto("standardGeneric", &[arg("f"), arg("fdef")], false),
    proto("storage.mode<-", &[arg("x"), arg("value")], false),
    proto("substitute", &[arg("expr"), arg("env")], false),
    proto("switch", &[arg("EXPR"), arg("...")], false),
    proto("tracemem", X, false),
    proto("unCfillPOSIXlt", X, false),
    proto("unclass", X, false),
    proto("untracemem", X, false),
    proto("Exec", &[arg("expr"), arg("envir")], false),
    proto("Tailcall", &[arg("FUN"), arg("...")], false),
];

const GENERIC_PROTOTYPES: &[PrimitivePrototype] = &[
    proto(
        "anyNA",
        &[arg("x"), arg_default("recursive", FormalDefault::False)],
        true,
    ),
    proto("as.character", &[arg("x"), arg("...")], true),
    proto("as.complex", &[arg("x"), arg("...")], true),
    proto("as.double", &[arg("x"), arg("...")], true),
    proto("as.environment", X, true),
    proto("as.integer", &[arg("x"), arg("...")], true),
    proto("as.list", &[arg("x"), arg("...")], true),
    proto("as.logical", &[arg("x"), arg("...")], true),
    proto("as.call", X, true),
    proto("as.numeric", &[arg("x"), arg("...")], true),
    proto("as.raw", X, true),
    proto("c", DOTS, true),
    proto("dim", X, true),
    proto("dim<-", &[arg("x"), arg("value")], true),
    proto("dimnames", X, true),
    proto("dimnames<-", &[arg("x"), arg("value")], true),
    proto("is.array", X, true),
    proto("is.finite", X, true),
    proto("is.infinite", X, true),
    proto("is.matrix", X, true),
    proto("is.na", X, true),
    proto("is.nan", X, true),
    proto("is.numeric", X, true),
    proto("length", X, true),
    proto("length<-", &[arg("x"), arg("value")], true),
    proto("levels<-", &[arg("x"), arg("value")], true),
    proto(
        "log",
        &[arg("x"), arg_default("base", FormalDefault::ExpOne)],
        true,
    ),
    proto("log2", X, true),
    proto("log10", X, true),
    proto("names", X, true),
    proto("names<-", &[arg("x"), arg("value")], true),
    proto("rep", &[arg("x"), arg("...")], true),
    proto(
        "seq.int",
        &[
            arg("from"),
            arg("to"),
            arg("by"),
            arg("length.out"),
            arg("along.with"),
            arg("..."),
        ],
        true,
    ),
    proto("xtfrm", X, true),
    proto("abs", X, true),
    proto("sign", X, true),
    proto("sqrt", X, true),
    proto("floor", X, true),
    proto("ceiling", X, true),
    proto("exp", X, true),
    proto("expm1", X, true),
    proto("log1p", X, true),
    proto("cos", X, true),
    proto("sin", X, true),
    proto("tan", X, true),
    proto("acos", X, true),
    proto("asin", X, true),
    proto("atan", X, true),
    proto("cosh", X, true),
    proto("sinh", X, true),
    proto("tanh", X, true),
    proto("acosh", X, true),
    proto("asinh", X, true),
    proto("atanh", X, true),
    proto("cospi", X, true),
    proto("sinpi", X, true),
    proto("tanpi", X, true),
    proto("gamma", X, true),
    proto("lgamma", X, true),
    proto("digamma", X, true),
    proto("trigamma", X, true),
    proto("cumsum", X, true),
    proto("cumprod", X, true),
    proto("cummax", X, true),
    proto("cummin", X, true),
    proto("cumvar", X, true),
    proto("+", E1_E2, true),
    proto("-", E1_E2, true),
    proto("*", E1_E2, true),
    proto("/", E1_E2, true),
    proto("^", E1_E2, true),
    proto("%%", E1_E2, true),
    proto("%/%", E1_E2, true),
    proto("&", E1_E2, true),
    proto("|", E1_E2, true),
    proto("==", E1_E2, true),
    proto("!=", E1_E2, true),
    proto("<", E1_E2, true),
    proto("<=", E1_E2, true),
    proto(">=", E1_E2, true),
    proto(">", E1_E2, true),
    proto("!", X, true),
    proto("%*%", &[arg("x"), arg("y")], true),
    proto("crossprod", X_Y, true),
    proto("tcrossprod", X_Y, true),
    proto("all", SUMMARIES, true),
    proto("any", SUMMARIES, true),
    proto("sum", SUMMARIES, true),
    proto("prod", SUMMARIES, true),
    proto("max", SUMMARIES, true),
    proto("min", SUMMARIES, true),
    proto("range", SUMMARIES, true),
    proto("Arg", Z, true),
    proto("Conj", Z, true),
    proto("Im", Z, true),
    proto("Mod", Z, true),
    proto("Re", Z, true),
    proto(
        "round",
        &[
            arg("x"),
            arg_default("digits", FormalDefault::Int(0)),
            arg("..."),
        ],
        true,
    ),
    proto(
        "signif",
        &[arg("x"), arg_default("digits", FormalDefault::Int(6))],
        true,
    ),
    proto("trunc", &[arg("x"), arg("...")], true),
];

const fn proto(
    name: &'static str,
    formals: &'static [FormalSpec],
    generic: bool,
) -> PrimitivePrototype {
    PrimitivePrototype {
        name,
        formals,
        generic,
    }
}

pub(crate) const LANGUAGE_ELEMENTS: &[&str] = &[
    "(", "{", ":", "~", "<-", "<<-", "=", "[", "[[", "[[<-", "[<-", "@", "@<-", "$", "$<-", "&&",
    "||", "break", "for", "function", "if", "next", "repeat", "return", "while",
];

unsafe fn initialize_primitive_metadata_in(base_env: SEXP) {
    unsafe {
        let args_env = super::memory_ext::NewEnvironment(R_NilValue(), R_EmptyEnv(), R_NilValue());
        let generic_args_env =
            super::memory_ext::NewEnvironment(R_NilValue(), R_EmptyEnv(), R_NilValue());

        install_prototypes(args_env, base_env, NON_GENERIC_PROTOTYPES);
        install_prototypes(generic_args_env, base_env, GENERIC_PROTOTYPES);

        // Every primitive checks these registries, including absent names.
        // Index their canonical frames even below the general size threshold;
        // the existing index reads live cells and invalidates structural edits.
        super::env_hash::promote_to_hash_table(args_env);
        super::env_hash::promote_to_hash_table(generic_args_env);

        defineVar(Rf_install_in_current(".ArgsEnv"), args_env, base_env);
        defineVar(
            Rf_install_in_current(".GenericArgsEnv"),
            generic_args_env,
            base_env,
        );
    }
}

unsafe fn install_prototypes(target_env: SEXP, base_env: SEXP, prototypes: &[PrimitivePrototype]) {
    unsafe {
        for prototype in prototypes {
            if !base_binding_is_primitive(base_env, prototype.name) {
                continue;
            }
            let closure = prototype_closure(*prototype, base_env);
            defineVar(Rf_install_in_current(prototype.name), closure, target_env);
        }
    }
}

unsafe fn base_binding_is_primitive(base_env: SEXP, name: &str) -> bool {
    unsafe {
        let value = R_findVarInFrame(base_env, Rf_install_in_current(name));
        value != R_UnboundValue()
            && (TYPEOF(value) == SEXPTYPE::BUILTINSXP || TYPEOF(value) == SEXPTYPE::SPECIALSXP)
    }
}

unsafe fn prototype_closure(prototype: PrimitivePrototype, base_env: SEXP) -> SEXP {
    unsafe {
        let formals = formals_from_specs(prototype.formals);
        let body = if prototype.generic {
            Rf_lang2(
                Rf_install_in_current("UseMethod"),
                string_scalar(prototype.name),
            )
        } else {
            R_NilValue()
        };
        crate::mainutils::dstruct::mkCLOSXP(formals, body, base_env)
    }
}

/// Names GNU R accounts as primitives (ArgsEnv + GenericArgsEnv + langElts).
pub fn is_accounted_primitive_name(name: &str) -> bool {
    LANGUAGE_ELEMENTS.contains(&name)
        || NON_GENERIC_PROTOTYPES.iter().any(|p| p.name == name)
        || GENERIC_PROTOTYPES.iter().any(|p| p.name == name)
}

/// GNU internal generics: only these primitives UseMethod on classed args.
pub fn is_internal_generic_name(name: &str) -> bool {
    GENERIC_PROTOTYPES.iter().any(|p| p.name == name)
}

unsafe fn formals_from_specs(specs: &[FormalSpec]) -> SEXP {
    unsafe {
        let formals = Rf_allocList(specs.len() as i32);
        let mut cell = formals;
        for spec in specs {
            super::accessors::SETCAR(cell, formal_default_value(spec.default));
            SETTAG(cell, Rf_install_in_current(spec.name));
            cell = super::accessors::CDR(cell);
        }
        formals
    }
}

unsafe fn formal_default_value(default: FormalDefault) -> SEXP {
    unsafe {
        match default {
            FormalDefault::Missing => R_MissingArg(),
            FormalDefault::Null => R_NilValue(),
            FormalDefault::False => Rf_ScalarLogical(FALSE),
            FormalDefault::True => Rf_ScalarLogical(TRUE),
            FormalDefault::Int(value) => Rf_ScalarInteger(value),
            FormalDefault::String(value) => string_scalar(value),
            FormalDefault::ExpOne => Rf_lang2(Rf_install_in_current("exp"), Rf_ScalarInteger(1)),
        }
    }
}

unsafe fn string_scalar(value: &str) -> SEXP {
    unsafe {
        let c_value = CString::new(value).expect("static R string has no interior NUL");
        Rf_mkString(c_value.as_ptr())
    }
}

unsafe fn Rf_install_in_current(name: &str) -> SEXP {
    unsafe {
        let c_name = CString::new(name).expect("static R symbol name has no interior NUL");
        super::symbol::Rf_install(c_name.as_ptr())
    }
}

unsafe fn pre_intern_symbols() {
    with_required_current_instance(|inst| unsafe { pre_intern_symbols_in(inst) });
}

unsafe fn pre_intern_symbols_in(inst: *mut RInstance) {
    unsafe {
        let pin = initialization_owner_pin(inst);
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
            require_initialization_live(&pin);
        }
    }
}

pub unsafe fn shutdown_r() {
    with_required_current_instance(shutdown_r_in);
}

pub(crate) fn shutdown_r_in(inst: *mut RInstance) {
    // P2: single-field write; no other raw path touches the instance here.
    unsafe {
        (*inst).initialized = false;
    }
}

mod bindings_containers;
mod bindings_core;
mod bindings_datetime;
mod bindings_format;
mod bindings_misc;
mod bindings_ops;

#[cfg(test)]
mod tests;
