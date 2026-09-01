//! R interpreter initialization.
//!
//! Initializes the active session's base bindings and common symbols. The
//! environment chain itself is owned by `RInstance`; there is intentionally no
//! process-global fallback interpreter.

use super::accessors::{CDR, SETCAR, SETTAG, TYPEOF};
use super::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_allocList, Rf_lang2, Rf_lang3, Rf_lang4, Rf_mkString,
};
use super::envir::{R_findVarInFrame, defineVar};
use super::ffi::{FALSE, SEXP, SEXPTYPE, TRUE};
use super::globals::{R_EmptyEnv, R_MissingArg, R_NilValue, R_UnboundValue};
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

        crate::eval::arithmetic::register_special_forms(base_env);
        crate::mainutils::essentials::register_essentials_builtins(base_env);
        initialize_special_environment_bindings(base_env);
        crate::mainutils::machine::Init_R_Machine(base_env);
        crate::mainutils::options::InitOptions();
        initialize_base_functions(base_env);
        initialize_primitive_metadata_in(base_env);
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
        // `%||%` <- function(x, y) if (is.null(x)) y else x
        let formals = formals_from_specs(&[arg("x"), arg("y")]);
        let _formals_guard = super::protect::protect(formals);

        let x = Rf_install_in_current("x");
        let y = Rf_install_in_current("y");
        let condition = Rf_lang2(Rf_install_in_current("is.null"), x);
        let _condition_guard = super::protect::protect(condition);
        let body = Rf_lang4(Rf_install_in_current("if"), condition, y, x);
        let _body_guard = super::protect::protect(body);
        let closure = crate::mainutils::dstruct::mkCLOSXP(formals, body, base_env);
        let _closure_guard = super::protect::protect(closure);

        defineVar(Rf_install_in_current("%||%"), closure, base_env);

        // `identical` is an ordinary base closure in GNU R, not the internal
        // primitive itself. Keeping that wrapper matters for argument
        // matching: an explicitly empty actual such as `identical(x, y,)`
        // selects `num.eq = TRUE` before `.Internal(identical(...))` eagerly
        // evaluates its complete argument list.
        let identical_formals = formals_from_specs(&[
            arg("x"),
            arg("y"),
            arg_default("num.eq", FormalDefault::True),
            arg_default("single.NA", FormalDefault::True),
            arg_default("attrib.as.set", FormalDefault::True),
            arg_default("ignore.bytecode", FormalDefault::True),
            arg_default("ignore.environment", FormalDefault::False),
            arg_default("ignore.srcref", FormalDefault::True),
            arg_default("extptr.as.ref", FormalDefault::False),
        ]);
        let _identical_formals_guard = super::protect::protect(identical_formals);

        // Build `identical(x, y, num.eq, ..., extptr.as.ref)` as a language
        // object, then wrap it in `.Internal(...)`. The internal lookup uses
        // R_FunTab, so replacing the public base binding with this closure
        // does not hide the primitive implementation.
        let internal_call = Rf_allocList(10);
        let _internal_call_guard = super::protect::protect(internal_call);
        (*internal_call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
        let mut cell = internal_call;
        for name in [
            "identical",
            "x",
            "y",
            "num.eq",
            "single.NA",
            "attrib.as.set",
            "ignore.bytecode",
            "ignore.environment",
            "ignore.srcref",
            "extptr.as.ref",
        ] {
            SETCAR(cell, Rf_install_in_current(name));
            cell = CDR(cell);
        }
        let identical_body = Rf_lang2(Rf_install_in_current(".Internal"), internal_call);
        let _identical_body_guard = super::protect::protect(identical_body);
        let identical_closure =
            crate::mainutils::dstruct::mkCLOSXP(identical_formals, identical_body, base_env);
        let _identical_closure_guard = super::protect::protect(identical_closure);

        defineVar(
            Rf_install_in_current("identical"),
            identical_closure,
            base_env,
        );

        // `I` <- function(x) {
        //     class(x) <- unique(c("AsIs", oldClass(x)))
        //     x
        // }
        //
        // GNU R defines this wrapper in base's dataframe.R.  It must prepend
        // rather than replace an existing explicit class, and `unique` keeps
        // repeated I() calls idempotent.  Keep it as a closure so assigning
        // the class follows the evaluator's normal copy-on-modify path.
        let as_is_formals = formals_from_specs(&[arg("x")]);
        let _as_is_formals_guard = super::protect::protect(as_is_formals);

        let as_is = Rf_mkString(c"AsIs".as_ptr());
        let _as_is_guard = super::protect::protect(as_is);
        let old_class = Rf_lang2(Rf_install_in_current("oldClass"), x);
        let _old_class_guard = super::protect::protect(old_class);
        let classes = Rf_lang3(Rf_install_in_current("c"), as_is, old_class);
        let _classes_guard = super::protect::protect(classes);
        let unique_classes = Rf_lang2(Rf_install_in_current("unique"), classes);
        let _unique_classes_guard = super::protect::protect(unique_classes);
        let class_lhs = Rf_lang2(Rf_install_in_current("class"), x);
        let _class_lhs_guard = super::protect::protect(class_lhs);
        let class_assignment = Rf_lang3(Rf_install_in_current("<-"), class_lhs, unique_classes);
        let _class_assignment_guard = super::protect::protect(class_assignment);
        let as_is_body = Rf_lang3(Rf_install_in_current("{"), class_assignment, x);
        let _as_is_body_guard = super::protect::protect(as_is_body);
        let as_is_closure =
            crate::mainutils::dstruct::mkCLOSXP(as_is_formals, as_is_body, base_env);
        let _as_is_closure_guard = super::protect::protect(as_is_closure);

        defineVar(Rf_install_in_current("I"), as_is_closure, base_env);
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
    proto("pairlist", DOTS, false),
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
    proto("as.logical", &[arg("x"), arg("...")], true),
    proto("as.pairlist", &[arg("x"), arg("...")], true),
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
    fn test_null_coalescing_base_function_is_lazy_and_null_specific() {
        let mut session = crate::sexp::session::RSession::new();

        let (result, _, _) = session.eval_code_with_output_capture("NULL %||% 42L");
        assert_eq!(
            result
                .expect("NULL should select the fallback")
                .integer_elt(0),
            Some(42)
        );

        let (result, _, _) =
            session.eval_code_with_output_capture("1L %||% missing_fallback_must_not_be_forced");
        assert_eq!(
            result
                .expect("a non-NULL left operand must not force the fallback")
                .integer_elt(0),
            Some(1)
        );

        let (result, _, _) = session.eval_code_with_output_capture("FALSE %||% 42L");
        assert_eq!(
            result
                .expect("false is a value, not a null operand")
                .logical_elt(0),
            Some(FALSE)
        );
    }

    #[test]
    fn test_as_is_base_function_preserves_and_prepends_explicit_classes() {
        let mut session = crate::sexp::session::RSession::new();

        let (result, _, _) = session.eval_code_with_output_capture(
            r#"
                x <- structure(1:3, names = c("a", "b", "c"), class = c("foo", "AsIs", "bar"))
                y <- I(x)
                identical(oldClass(y), c("AsIs", "foo", "bar")) &&
                    identical(names(y), names(x)) &&
                    identical(oldClass(I(y)), oldClass(y))
            "#,
        );
        assert_eq!(
            result
                .expect("I() should preserve attributes and prepend one AsIs class")
                .logical_elt(0),
            Some(TRUE)
        );

        let (result, _, _) = session.eval_code_with_output_capture(
            r#"
                y <- I(matrix(1:4, 2L, 2L))
                identical(oldClass(y), "AsIs") && identical(dim(y), c(2L, 2L))
            "#,
        );
        assert_eq!(
            result
                .expect("I() should add AsIs without discarding unrelated attributes")
                .logical_elt(0),
            Some(TRUE)
        );
    }

    #[test]
    fn test_initialize_installs_machine_constants() {
        let mut session = crate::sexp::session::RSession::new();

        let (result, _, _) = session.eval_code_with_output_capture(".Machine$double.eps");
        assert_eq!(
            result
                .expect(".Machine should be installed in the base environment")
                .real_elt(0),
            Some(f64::EPSILON)
        );
    }

    #[test]
    fn test_initialize_installs_live_options_binding() {
        let mut session = crate::sexp::session::RSession::new();

        let (result, _, _) = session.eval_code_with_output_capture(".Options$width");
        assert_eq!(
            result
                .expect(".Options should be installed in the base environment")
                .integer_elt(0),
            Some(80)
        );

        let (result, _, _) =
            session.eval_code_with_output_capture("options(rErr.eps = 1e-30); .Options$rErr.eps");
        assert_eq!(
            result
                .expect("options() should refresh the .Options binding")
                .real_elt(0),
            Some(1e-30)
        );
    }

    #[test]
    fn test_options_binding_refresh_survives_gc_torture() {
        let mut session = crate::sexp::session::RSession::new();

        let (result, _, _) = session.eval_code_with_output_capture(
            "gctorture(TRUE); options(alpha = 20L, beta = 22L); value <- .Options$alpha + .Options$beta; gctorture(FALSE); value",
        );
        assert_eq!(
            result
                .expect("the refreshed .Options pairlist should remain rooted")
                .integer_elt(0),
            Some(42)
        );
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
