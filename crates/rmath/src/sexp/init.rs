//! R interpreter initialization.
//!
//! Initializes the active session's base bindings and common symbols. The
//! environment chain itself is owned by `RInstance`; there is intentionally no
//! process-global fallback interpreter.

use super::accessors::{CDR, SETCAR, SETTAG, SET_SYMVALUE, TYPEOF};

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
        super::context::install_r_panic_hook();
        // P1: raw place accesses — initialize_base_bindings_in reenters the
        // interpreter (symbol interning, protect pushes, builtin tables).
        if !(*inst).initialized {
            let base_env = (*inst).base_env;
            initialize_base_bindings_in(inst, base_env);
            (*inst).initialized = true;
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
        initialize_base_bindings_in(instance, base_env);
    }
}

pub(crate) unsafe fn initialize_base_bindings_in(inst: *mut RInstance, base_env: SEXP) {
    unsafe {
        let _scope = ScopedCurrentInstance::install(inst);

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
        // GNU base binds `.BaseNamespaceEnv` to namespace:base. rport's
        // base environment is that namespace; methods extraS4 wrappers
        // (unlist, as.vector, lengths) must live here so implicitGeneric
        // treats them as base functions, not primitives.
        defineVar(
            Rf_install_in_current(".BaseNamespaceEnv"),
            base_env,
            base_env,
        );

        // GNU formals.R: alist <- function(...) as.list(sys.call())[-1L]
        // Installed after as.list so parse/eval can see the generic.


        // GNU: as.list <- function(x, ...) UseMethod("as.list")
        // The builtin stays as as.list.default; as.list.function is a
        // separate method so as.list(sum) is list(NULL) and as.list(as.list)
        // keeps the generic formals.
        let as_list_formals = formals_from_specs(&[arg("x"), arg("...")]);
        let _as_list_formals_guard = super::protect::protect(as_list_formals);
        let as_list_generic = Rf_mkString(c"as.list".as_ptr());
        let _as_list_generic_guard = super::protect::protect(as_list_generic);
        let as_list_body = Rf_lang2(Rf_install_in_current("UseMethod"), as_list_generic);
        let _as_list_body_guard = super::protect::protect(as_list_body);
        let as_list_closure =
            crate::mainutils::dstruct::mkCLOSXP(as_list_formals, as_list_body, base_env);
        let _as_list_closure_guard = super::protect::protect(as_list_closure);
        defineVar(Rf_install_in_current("as.list"), as_list_closure, base_env);
        eval_base_binding(
            base_env,
            "as.list.default",
            "function (x, ...) if (typeof(x) == \"list\") x else .Internal(as.vector(x, \"list\"))",
        );

        // GNU formals.R is `as.list(sys.call())[-1L]`. sys.call() forced as
        // as.list's argument still sees the as.list frame (rport-qc8ct).
        // Evaluate sys.call in alist first so missing formals stay missing.

        eval_base_binding(
            base_env,
            "alist",
            "function(...) { sc <- sys.call(); as.list(sc)[-1L] }",
        );




        // GNU pairlist.R: closures over .Internal(as.vector(..., "pairlist")).
        eval_base_binding(
            base_env,
            "as.pairlist",
            "function(x) .Internal(as.vector(x, \"pairlist\"))",
        );
        eval_base_binding(
            base_env,
            "pairlist",
            "function(...) as.pairlist(list(...))",
        );
        // GNU as.R / unlist.R / New-Internal.R: these are closures, not
        // primitives. methods::.BasicFunsList still lists them so setMethod
        // can wrap the closure (primitives.R extraS4).
        eval_base_binding(
            base_env,
            "as.vector",
            "function(x, mode = \"any\") .Internal(as.vector(x, mode))",
        );
        eval_base_binding(
            base_env,
            "lengths",
            "function(x, use.names = TRUE) .Internal(lengths(x, use.names))",
        );
        eval_base_binding(
            base_env,
            "unlist",
            "function(x, recursive = TRUE, use.names = TRUE) {\n\
             if (is.function(recursive) || length(recursive) != 1L)\n\
                 stop(\"'recursive' must be a length-1 vector\")\n\
             if (is.na(recursive)) stop(\"'recursive' is NA\")\n\
             if (.Internal(islistfactor(x, recursive))) {\n\
                 URapply <- if (recursive)\n\
                     function(x, Fn) .Internal(unlist(rapply(x, Fn, how = \"list\"), recursive, FALSE))\n\
                 else function(x, Fn) .Internal(unlist(lapply(x, Fn), recursive, FALSE))\n\
                 lv <- unique(URapply(x, levels))\n\
                 nm <- if (use.names) names(.Internal(unlist(x, recursive, use.names)))\n\
                 res <- match(URapply(x, as.character), lv)\n\
                 structure(res, levels = lv, names = nm, class = \"factor\")\n\
             } else .Internal(unlist(x, recursive, use.names))\n\
             }",
        );

        // GNU formals.R: replacement functions are closures, not primitives.
        eval_base_binding(
            base_env,
            "body<-",
            "function (fun, envir = environment(fun), value) {\n\
             if (!is.function(fun)) warning(\"'fun' is not a function\")\n\
             if (is.expression(value)) {\n\
                 if (length(value) > 1L)\n\
                     warning(\"using the first element of 'value' of type \\\"expression\\\"\")\n\
                 value <- value[[1L]]\n\
             }\n\
             as.function(c(formals(fun),\n\
                 if (is.null(value) || is.atomic(value) || is.list(value)) list(value) else value),\n\
                 envir)\n\
             }",
        );
        eval_base_binding(
            base_env,
            "formals<-",
            "function (fun, envir = environment(fun), value) {\n\
             if (!is.function(fun)) warning(\"'fun' is not a function\")\n\
             bd <- body(fun)\n\
             as.function(c(value,\n\
                 if (is.null(bd) || is.atomic(bd) || is.list(bd)) list(bd) else bd),\n\
                 envir)\n\
             }",
        );



        // GNU apply.R: n-d arrays, empty-extent MARGIN, and FUN=NULL collapse.
        eval_base_binding(base_env, "apply", include_str!("gnu_apply.R"));
        eval_base_binding(
            base_env,
            "determinant",
            "function(x, logarithm = TRUE, ...) UseMethod(\"determinant\")",
        );
        eval_base_binding(
            base_env,
            "determinant.matrix",
            "function(x, logarithm = TRUE, ...) {\n\
             if ((n <- ncol(x)) != nrow(x))\n\
                 stop(\"'x' must be a square matrix\")\n\
             if (n < 1L)\n\
                 return(structure(list(modulus = structure(if (logarithm) 0 else 1, logarithm = logarithm), sign = 1L), class = \"det\"))\n\
             if (is.complex(x))\n\
                 stop(\"'determinant' not currently defined for complex matrices\")\n\
             det_ge_real(x, logarithm)\n\
             }",
        );
        eval_base_binding(
            base_env,
            "det",
            "function(x, ...) { z <- determinant(x, logarithm = TRUE, ...); c(z$sign * exp(z$modulus)) }",
        );
        eval_base_binding(
            base_env,
            "La.svd",
            "function(x, nu = min(n, p), nv = min(n, p)) {\n\
             if (!is.logical(x) && !is.numeric(x) && !is.complex(x))\n\
                 stop(\"argument to 'La.svd' must be numeric or complex\")\n\
             if (any(!is.finite(x))) stop(\"infinite or missing values in 'x'\")\n\
             x <- as.matrix(x)\n\
             n <- nrow(x); p <- ncol(x)\n\
             if (!n || !p) stop(\"a dimension is zero\")\n\
             zero <- if (is.complex(x)) 0+0i else 0\n\
             if (nu || nv) {\n\
                 np <- min(n, p)\n\
                 if (nu <= np && nv <= np) {\n\
                     jobu <- \"S\"; u <- matrix(zero, n, np); vt <- matrix(zero, np, p); nu0 <- nv0 <- np\n\
                 } else {\n\
                     jobu <- \"A\"; u <- matrix(zero, n, n); vt <- matrix(zero, p, p); nu0 <- n; nv0 <- p\n\
                 }\n\
             } else {\n\
                 jobu <- \"N\"; u <- matrix(zero, 1L, 1L); vt <- matrix(zero, 1L, 1L); nu0 <- nv0 <- 0L\n\
             }\n\
             res <- if (is.complex(x)) La_svd_cmplx(jobu, x, double(min(n, p)), u, vt) else La_svd(jobu, x, double(min(n, p)), u, vt)\n\
             res <- res[c(\"d\", if (nu) \"u\", if (nv) \"vt\")]\n\
             if (nu && nu < nu0) res$u <- res$u[, seq_len(min(n, nu)), drop = FALSE]\n\
             if (nv && nv < nv0) res$vt <- res$vt[seq_len(min(p, nv)), drop = FALSE]\n\
             res\n\
             }",
        );
        // GNU kappa.R: public norm/rcond are closures (implicit generics).
        eval_base_binding(
            base_env,
            "norm",
            "function(x, type = c(\"O\", \"I\", \"F\", \"M\", \"2\")) {\n\
             if (identical(\"2\", type)) {\n\
                 if (!length(x)) 0 else if (anyNA(x)) NA_real_ else svd(x, nu = 0L, nv = 0L)$d[1L]\n\
             } else if (is.numeric(x) || is.logical(x))\n\
                 .Internal(La_dlange(x, type))\n\
             else if (is.complex(x))\n\
                 .Internal(La_zlange(x, type))\n\
             else stop(sprintf(\"invalid 'x': type \\\"%s\\\"\", typeof(x)))\n\
             }",
        );
        eval_base_binding(
            base_env,
            "rcond",
            "function(x, norm = c(\"O\", \"I\", \"1\"), triangular = FALSE, uplo = \"U\", ...) {\n\
             norm <- match.arg(norm)\n\
             stopifnot(length(d <- dim(x)) == 2L)\n\
             if (!all(d)) return(1 / 0)\n\
             if (d[1L] != d[2L])\n\
                 return(rcond(qr.R(qr(if (d[1L] < d[2L]) t(x) else x)), triangular = TRUE, uplo = \"U\", norm = norm, ...))\n\
             if (is.complex(x)) {\n\
                 if (triangular) switch(uplo,\n\
                     \"U\" = .Internal(La_ztrcon(x, norm)),\n\
                     \"L\" = .Internal(La_ztrcon3(x, norm, \"L\")),\n\
                     stop(\"'uplo' must be \\\"U\\\" or \\\"L\\\"\"))\n\
                 else .Internal(La_zgecon(x, norm))\n\
             } else {\n\
                 if (triangular) switch(uplo,\n\
                     \"U\" = .Internal(La_dtrcon(x, norm)),\n\
                     \"L\" = .Internal(La_dtrcon3(x, norm, \"L\")),\n\
                     stop(\"'uplo' must be \\\"U\\\" or \\\"L\\\"\"))\n\
                 else .Internal(La_dgecon(x, norm))\n\
             }\n\
             }",
        );
        eval_base_binding(
            base_env,
            ".getNamespace",
            "function(name) .Internal(getRegisteredNamespace(name))",
        );
        eval_base_binding(
            base_env,
            "..getNamespace",
            "function(name, where) {\n\
             .Internal(getRegisteredNamespace(name)) %||% tryCatch(loadNamespace(name), error = function(e) .GlobalEnv)\n\
             }",
        );
        // GNU namespace.R: isBaseNamespace(ns) identical(ns, .BaseNamespaceEnv).
        // methods::show,genericFunction-method calls .minimalName → this.
        eval_base_binding(
            base_env,
            "isBaseNamespace",
            "function(ns) identical(ns, .BaseNamespaceEnv)",
        );
        // GNU namespace.R: methods::.isExported and show() need these.
        eval_base_binding(
            base_env,
            ".getNamespaceInfo",
            "function(ns, which) {\n\
             info <- get(\".__NAMESPACE__.\", envir = ns, inherits = FALSE)\n\
             get(which, envir = info, inherits = FALSE)\n\
             }",
        );
        eval_base_binding(
            base_env,
            "getNamespaceInfo",
            "function(ns, which) {\n\
             ns <- asNamespace(ns)\n\
             .getNamespaceInfo(ns, which)\n\
             }",
        );
        eval_base_binding(
            base_env,
            "setNamespaceInfo",
            "function(ns, which, val) {\n\
             ns <- asNamespace(ns)\n\
             info <- get(\".__NAMESPACE__.\", envir = ns, inherits = FALSE)\n\
             assign(which, val, envir = info)\n\
             }",
        );
        eval_base_binding(
            base_env,
            ".rmpkg",
            "function(pkg) sub(\"package:\", \"\", pkg, fixed=TRUE)",
        );
        // GNU stats::toeplitz is a closure (diffinv.R), not a primitive.
        // Kernel is hidden `.rport_toeplitz`; public formals match GNU
        // (no `...`). methods onLoad re-caches `.__IG__table` and points
        // this closure at the stats namespace so implicitGeneric finds
        // GNU's package="stats" `function(x, ...)` entry.

        eval_base_binding(
            base_env,
            "toeplitz",
            "function(x, r = NULL, symmetric = is.null(r))\n\
             .rport_toeplitz(x, r, symmetric)",
        );



        eval_base_binding(
            base_env,
            "trace",
            "function(what, tracer, exit, at, print, signature,\n\
             where = topenv(parent.frame()), edit = FALSE)\n\
             {\n\
             if(nargs() > 1L && !.isMethodsDispatchOn()) {\n\
             ns <- try(loadNamespace(\"methods\"))\n\
             if(isNamespace(ns))\n\
             message(\"(loaded the methods namespace)\", domain = NA)\n\
             else\n\
             stop(\"tracing functions requires the 'methods' package, but unable to load the 'methods' namespace\")\n\
             }\n\
             else if(nargs() == 1L)\n\
             return(.primTrace(what))\n\
             tState <- tracingState(FALSE)\n\
             on.exit(tracingState(tState))\n\
             call <- sys.call()\n\
             call[[1L]] <- quote(methods:::.TraceWithMethods)\n\
             call$where <- where\n\
             eval.parent(call)\n\
             }",
        );
        eval_base_binding(
            base_env,
            "untrace",
            "function(what, signature = NULL, where = topenv(parent.frame())) {\n\
             if(!.isMethodsDispatchOn())\n\
             return(.primUntrace(what))\n\
             tState <- tracingState(FALSE)\n\
             on.exit(tracingState(tState))\n\
             call <- sys.call()\n\
             call[[1L]] <- quote(methods:::.TraceWithMethods)\n\
             call$where <- where\n\
             call$untrace <- TRUE\n\
             invisible(eval.parent(call))\n\
             }",
        );
        eval_base_binding(
            base_env,
            ".doTrace",
            "function(expr, msg) {\n\
             on <- tracingState(FALSE)\n\
             if(on) {\n\
             on.exit(tracingState(TRUE))\n\
             if(!missing(msg)) {\n\
             call <- deparse(sys.call(sys.parent(1L)))\n\
             if(length(call) > 1L)\n\
             call <- paste(call[[1]], \"....\")\n\
             cat(\"Tracing\", call, msg, \"\\n\")\n\
             }\n\
             exprObj <- substitute(expr)\n\
             eval.parent(exprObj)\n\
             }\n\
             NULL\n\
             }",
        );








        eval_base_binding(
            base_env,
            "matrix",
            "function(data = NA, nrow = NULL, ncol = NULL, byrow = FALSE, dimnames = NULL) {\n\
             if (is.object(data) || !is.atomic(data)) data <- as.vector(data)\n\
             matrix_impl(data, nrow, ncol, byrow, dimnames)\n\
             }",
        );





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

        // `chooseOpsMethod` is the base S3 generic consulted by
        // DispatchGroup when two distinct Ops methods are found.  The full
        // base package supplies this closure; install its small generic
        // definition here so embedded sessions get the same hook.
        let choose_formals = formals_from_specs(&[
            arg("x"),
            arg("y"),
            arg("mx"),
            arg("my"),
            arg("cl"),
            arg("reverse"),
        ]);
        let _choose_formals_guard = super::protect::protect(choose_formals);
        let choose_generic = Rf_mkString(c"chooseOpsMethod".as_ptr());
        let _choose_generic_guard = super::protect::protect(choose_generic);
        let choose_body = Rf_lang2(Rf_install_in_current("UseMethod"), choose_generic);
        let _choose_body_guard = super::protect::protect(choose_body);
        let choose_closure =
            crate::mainutils::dstruct::mkCLOSXP(choose_formals, choose_body, base_env);
        let _choose_closure_guard = super::protect::protect(choose_closure);
        defineVar(
            Rf_install_in_current("chooseOpsMethod"),
            choose_closure,
            base_env,
        );

        // The base default declines the conflict, allowing DispatchGroup to
        // issue its standard incompatible-method warning and use the default
        // arithmetic implementation.
        let choose_default_formals = formals_from_specs(&[
            arg("x"),
            arg("y"),
            arg("mx"),
            arg("my"),
            arg("cl"),
            arg("reverse"),
        ]);
        let _choose_default_formals_guard = super::protect::protect(choose_default_formals);
        let choose_default_body = Rf_ScalarLogical(FALSE);
        let _choose_default_body_guard = super::protect::protect(choose_default_body);
        let choose_default_closure = crate::mainutils::dstruct::mkCLOSXP(
            choose_default_formals,
            choose_default_body,
            base_env,
        );
        let _choose_default_closure_guard = super::protect::protect(choose_default_closure);
        defineVar(
            Rf_install_in_current("chooseOpsMethod.default"),
            choose_default_closure,
            base_env,
        );

        // GNU stats::setNames is a closure so `setNames(, nm)` uses the
        // `object = nm` default instead of evalList's empty-arg error.
        eval_base_binding(
            base_env,
            "setNames",
            "function(object = nm, nm) { names(object) <- nm; object }",
        );
        eval_base_binding(
            base_env,
            "lazyLoadDBexec",
            include_str!("gnu_lazyLoadDBexec.R"),
        );
        eval_base_binding(base_env, "lazyLoad", include_str!("gnu_lazyLoad.R"));
        eval_base_binding(
            base_env,
            "%notin%",
            "function(x, table) match(x, table, nomatch = 0L) == 0L",
        );



        // GNU datetime.R constructors: class + tzone/units only.
        eval_base_binding(
            base_env,
            ".POSIXlt",
            "function(xx, tz = NULL, cl = c(\"POSIXlt\", \"POSIXt\")) { class(xx) <- cl; attr(xx, \"tzone\") <- tz; xx }",
        );
        eval_base_binding(
            base_env,
            ".difftime",
            "function(xx, units, cl = \"difftime\") { class(xx) <- cl; attr(xx, \"units\") <- units; xx }",
        );
        eval_base_binding(
            base_env,
            "names.POSIXlt",
            "function(x) names(x$year)",
        );
        eval_base_binding(
            base_env,
            "length.POSIXlt",
            "function(x) max(lengths(unclass(x)))",
        );
        eval_base_binding(
            base_env,
            "names<-.POSIXlt",
            "function(x, value) { n <- length(x); yr <- x$year; if (length(yr) < n) x$year <- rep_len(yr, n); if (length(value) < n) value <- c(as.character(value), rep(NA_character_, n - length(value))); if (length(value) > n) value <- value[seq_len(n)]; names(x$year) <- value; x }",
        );
        {
            const LEAP: [f64; 27] = [
                78_796_800.0,
                94_694_400.0,
                126_230_400.0,
                157_766_400.0,
                189_302_400.0,
                220_924_800.0,
                252_460_800.0,
                283_996_800.0,
                315_532_800.0,
                362_793_600.0,
                394_329_600.0,
                425_865_600.0,
                489_024_000.0,
                567_993_600.0,
                631_152_000.0,
                662_688_000.0,
                709_948_800.0,
                741_484_800.0,
                773_020_800.0,
                820_454_400.0,
                867_715_200.0,
                915_148_800.0,
                1_136_073_600.0,
                1_230_768_000.0,
                1_341_100_800.0,
                1_435_708_800.0,
                1_483_228_800.0,
            ];
            let leap = crate::sexp::constructors::Rf_allocVector3(
                crate::sexp::ffi::SEXPTYPE::REALSXP,
                LEAP.len() as i64,
            );
            let _l = super::protect::protect(leap);
            for (i, sec) in LEAP.iter().enumerate() {
                *crate::sexp::accessors::REAL(leap).add(i) = *sec;
            }
            let klass = crate::sexp::constructors::Rf_allocVector3(
                crate::sexp::ffi::SEXPTYPE::STRSXP,
                2,
            );
            let _k = super::protect::protect(klass);
            crate::sexp::accessors::SET_STRING_ELT(
                klass,
                0,
                crate::sexp::constructors::Rf_mkChar(c"POSIXct".as_ptr()),
            );
            crate::sexp::accessors::SET_STRING_ELT(
                klass,
                1,
                crate::sexp::constructors::Rf_mkChar(c"POSIXt".as_ptr()),
            );
            crate::sexp::attrib_core::R_classgets(leap, klass);
            defineVar(Rf_install_in_current(".leap.seconds"), leap, base_env);
        }

        // GNU New-Internal.R: deparse is a closure so warning() attributes
        // to `deparse(...)`, not the caller of the primitive. The 4th
        // `.Internal` argument stays the character `control` vector: our
        // bit layout is not GNU Defn.h's, so `.deparseOpts()` integers
        // cannot be forwarded yet.
        eval_base_binding(
            base_env,
            "deparse",
            r#"function(expr, width.cutoff = 60L,
         backtick = mode(expr) %in% c("call", "expression", "(", "function"),
         control = c("keepNA", "keepInteger", "niceNames", "showAttributes"),
         nlines = -1L)
    .Internal(deparse(expr, width.cutoff, backtick, control, nlines))"#,
        );
        // GNU cat.R: visible `cat` is a closure so deparse does not treat
        // it as a primitive (inlist++), matching stock if-as-arg wrapping.
        eval_base_binding(
            base_env,
            "cat",
            r#"function(..., file = "", sep = " ", fill = FALSE,
                labels = NULL, append = FALSE)
    .Internal(cat(list(...), file, sep, fill, labels, append))"#,
        );

        // GNU source.R: withAutoprint is a closure so body()/formals()
        // and eval-etc's withVisible(withAutoprint({...})) match stock.
        eval_base_binding(
            base_env,

            "withAutoprint",
            r#"function(exprs, evaluated = FALSE, local = parent.frame(),
                          print. = TRUE, echo = TRUE, max.deparse.length = Inf,
                          width.cutoff = max(20, getOption("width")),
                          deparseCtrl = c("keepInteger", "showAttributes", "keepNA"),
                          skip.echo = 0,
                          ...)
{
    if (!evaluated) {
        exprs <- substitute(exprs)
        if (is.call(exprs)) {
            if (exprs[[1]] == quote(`{`)) {
                exprs <- as.list(exprs)[-1]
                if (missing(skip.echo) && is.list(srcrefs <- attr(exprs, "srcref"))) {
                    skip.echo <- srcrefs[[1L]][7L] - 1L
                }
            }
        }
    }
    source(exprs = exprs, local = local, print.eval = print., echo = echo,
           max.deparse.length = max.deparse.length, width.cutoff = width.cutoff,
           deparseCtrl = deparseCtrl, skip.echo = skip.echo, ...)
}"#,
        );










    }
}


unsafe fn eval_base_binding(base_env: SEXP, name: &str, source: &str) {
    unsafe {
        let parsed = super::memory::with_arena(|arena| {
            crate::eval::parser::parse_expressions(source, arena)
        });
        crate::eval::parser::flush_literal_warnings();
        let Ok(exprs) = parsed else {
            return;
        };
        if exprs.len() != 1 {
            return;
        }
        let value = crate::eval::eval::Rf_eval(exprs[0], base_env);
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
    LANGUAGE_ELEMENTS.iter().any(|n| *n == name)
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

pub(crate) fn shutdown_r_in(inst: *mut RInstance) {
    // P2: single-field write; no other raw path touches the instance here.
    unsafe {
        (*inst).initialized = false;
    }
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
            assert!(crate::sexp::accessors::PRIMOFFSET(log_val) <= -2);
            assert_eq!(
                crate::eval::primitive::portable_primitive_name(
                    crate::sexp::object::Sexp::try_from_raw(log_val).unwrap()
                )
                .as_deref(),
                Some("log")
            );

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
