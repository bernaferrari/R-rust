//! R interpreter initialization.
//!
//! Initializes the active session's base bindings and common symbols. The
//! environment chain itself is owned by `RInstance`; there is intentionally no
//! process-global fallback interpreter.

use super::accessors::{CDR, SETCAR, SETTAG, SET_SYMVALUE, SYMVALUE, TYPEOF};

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

        // GNU: T and F are ordinary symbols bound to TRUE/FALSE, not parser
        // keywords. `quote(F())` and `substitute(F(), list(F=...))` need
        // the symbol, while bare `F` still evaluates to FALSE.
        let t = Rf_ScalarLogical(TRUE);
        let _t = super::protect::protect(t);
        defineVar(Rf_install_in_current("T"), t, base_env);
        let f = Rf_ScalarLogical(FALSE);
        let _f = super::protect::protect(f);
        defineVar(Rf_install_in_current("F"), f, base_env);
        let device = Rf_mkString(c"null device".as_ptr());
        let _device = super::protect::protect(device);
        defineVar(Rf_install_in_current(".Device"), device, base_env);
        let devices = super::constructors::Rf_allocVector(SEXPTYPE::VECSXP, 1);
        super::accessors::SET_VECTOR_ELT(devices, 0, Rf_mkString(c"null device".as_ptr()));
        defineVar(Rf_install_in_current(".Devices"), devices, base_env);
        let message_fn = crate::eval::primitive::make_primitive_binding("message", SEXPTYPE::BUILTINSXP);
        defineVar(Rf_install_in_current("message"), message_fn, base_env);
        let inherits_fn = crate::eval::primitive::make_primitive_binding("inherits", SEXPTYPE::BUILTINSXP);
        defineVar(Rf_install_in_current("inherits"), inherits_fn, base_env);

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
        // GNU New-Internal.R: NextMethod is a closure over .Internal so extra
        // named args land in `...` and CADDR(.Internal args) stays the dots
        // symbol. A primitive NextMethod would evaluate those extras and hit
        // "wrong argument ...".
        eval_base_binding(
            base_env,
            "NextMethod",
            "function(generic = NULL, object = NULL, ...)\n    .Internal(NextMethod(generic, object, ...))",
        );
        // GNU sweep.R: aperm(array(STATS, dims[MARGIN,...])) then FUN.
        // A primitive sweep recycled STATS along the wrong margin.
        eval_base_binding(
            base_env,
            "sweep",
            r#"function(x, MARGIN, STATS, FUN = "-", check.margin = TRUE, ...)
{
    FUN <- match.fun(FUN)
    dims <- dim(x)
    if (is.character(MARGIN)) {
        dn <- dimnames(x)
        if (is.null(dnn <- names(dn)))
            stop("'x' must have named dimnames")
        MARGIN <- match(MARGIN, dnn)
        if (anyNA(MARGIN))
            stop("not all elements of 'MARGIN' are names of dimensions")
    }
    if (check.margin) {
        dimmargin <- dims[MARGIN]
        dimstats <- dim(STATS)
        lstats <- length(STATS)
        if (lstats > prod(dimmargin)) {
            warning("STATS is longer than the extent of 'dim(x)[MARGIN]'")
        } else if (is.null(dimstats)) {
            cumDim <- c(1L, cumprod(dimmargin))
            upper <- min(cumDim[cumDim >= lstats])
            lower <- max(cumDim[cumDim <= lstats])
            if (lstats && (upper %% lstats != 0L || lstats %% lower != 0L))
                warning("STATS does not recycle exactly across MARGIN")
        } else {
            dimmargin <- dimmargin[dimmargin > 1L]
            dimstats <- dimstats[dimstats > 1L]
            if (length(dimstats) != length(dimmargin) ||
                any(dimstats != dimmargin))
                warning("length(STATS) or dim(STATS) do not match dim(x)[MARGIN]")
        }
    }
    perm <- c(MARGIN, seq_along(dims)[-MARGIN])
    FUN(x, aperm(array(STATS, dims[perm]), order(perm)), ...)
}"#,
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
        // GNU lapply.R: closure over .Internal(lapply), not a primitive.
        eval_base_binding(
            base_env,
            "lapply",
            "function (X, FUN, ...) {\n\
             FUN <- match.fun(FUN)\n\
             if (!is.vector(X) || is.object(X)) X <- as.list(X)\n\
             .Internal(lapply(X, FUN))\n\
             }",
        );
        // GNU sapply.R: vapply is .Internal; sapply is lapply + simplify2array.
        eval_base_binding(
            base_env,
            "vapply",
            "function (X, FUN, FUN.VALUE, ..., USE.NAMES = TRUE) {\n\
             FUN <- match.fun(FUN)\n\
             if (!is.vector(X) || is.object(X)) X <- as.list(X)\n\
             .Internal(vapply(X, FUN, FUN.VALUE, USE.NAMES))\n\
             }",
        );
        eval_base_binding(
            base_env,
            "sapply",
            "function (X, FUN, ..., simplify = TRUE, USE.NAMES = TRUE) {\n\
             FUN <- match.fun(FUN)\n\
             answer <- lapply(X = X, FUN = FUN, ...)\n\
             if (USE.NAMES && is.character(X) && is.null(names(answer)))\n\
                 names(answer) <- X\n\
             if (!isFALSE(simplify))\n\
                 simplify2array(answer, higher = (simplify == \"array\"))\n\
             else answer\n\
             }",
        );
        // GNU split.R / tapply.R: closures over .Internal(split), not primitives.
        eval_base_binding(
            base_env,
            ".NotYetUsed",
            "function(arg, error = TRUE) {\n\
             msg <- gettextf(\"argument '%s' is not used (yet)\", arg)\n\
             if (error) stop(msg, domain = NA, call. = FALSE)\n\
             else warning(msg, domain = NA, call. = FALSE)\n\
             }",
        );
        eval_base_binding(base_env, "split", "function(x, f, drop = FALSE, ...) UseMethod(\"split\")");
        eval_base_binding(
            base_env,
            "split.default",
            include_str!("gnu_split_default.R"),
        );
        eval_base_binding(base_env, "interaction", include_str!("gnu_interaction.R"));
        eval_base_binding(
            base_env,
            "split<-",
            "function(x, f, drop = FALSE, ..., value) UseMethod(\"split<-\")",
        );
        eval_base_binding(
            base_env,
            "split<-.default",
            "function(x, f, drop = FALSE, ..., value) {\n\
             ix <- split(seq_along(x), f, drop = drop, ...)\n\
             n <- length(value)\n\
             j <- 0\n\
             for (i in ix) {\n\
                 j <- j %% n + 1\n\
                 x[i] <- value[[j]]\n\
             }\n\
             x\n\
             }",
        );
        eval_base_binding(
            base_env,
            "split<-.data.frame",
            "function(x, f, drop = FALSE, ..., value) {\n\
             ix <- split(seq_len(nrow(x)), f, drop = drop, ...)\n\
             n <- length(value)\n\
             j <- 0\n\
             for (i in ix) {\n\
                 j <- j %% n + 1\n\
                 x[i, ] <- value[[j]]\n\
             }\n\
             x\n\
             }",
        );
        eval_base_binding(base_env, "unsplit", include_str!("gnu_unsplit.R"));
        eval_base_binding(base_env, "stripchart", "function(x, ...) UseMethod(\"stripchart\")");
        eval_base_binding(
            base_env,
            "stripchart.default",
            "function(x, method = \"overplot\", ...) invisible(NULL)",
        );
        eval_base_binding(
            base_env,
            "stripchart.formula",
            "function(x, data = NULL, ...) invisible(NULL)",
        );

        eval_base_binding(
            base_env,
            "split.data.frame",
            "function(x, f, drop = FALSE, ...) {\n\
             lapply(split(x = seq_len(nrow(x)), f = f, drop = drop, ...),\n\
                    function(ind) x[ind, , drop = FALSE])\n\
             }",
        );
        eval_base_binding(base_env, "tapply", include_str!("gnu_tapply.R"));
        eval_base_binding(base_env, "is.data.frame", "function(x) inherits(x, \"data.frame\")");
        eval_base_binding(
            base_env,
            ".set_row_names",
            "function(n) if (n > 0) c(NA_integer_, -n) else integer()",
        );
        eval_base_binding(
            base_env,
            ".row_names_info",
            "function(x, type = 1L) {\n\
             rn <- row.names.stored(x)\n\
             if (type == 0L) return(rn)\n\
             if (is.integer(rn) && length(rn) == 2L && is.na(rn[1L])) {\n\
                 if (type == 1L) rn[2L] else abs(rn[2L])\n\
             } else if (is.character(rn) || is.integer(rn)) length(rn) else 0L\n\
             }",
        );
        eval_base_binding(
            base_env,
            "I",
            "function(x) { class(x) <- unique.default(c(\"AsIs\", oldClass(x))); x }",
        );
        // GNU dataframe.R: closures, not primitives.
        eval_base_binding(
            base_env,
            "as.data.frame",
            "function(x, row.names = NULL, optional = FALSE, ...) {\n\
             if (is.null(x)) return(as.data.frame(list()))\n\
             UseMethod(\"as.data.frame\")\n\
             }",
        );
        eval_base_binding(
            base_env,
            "as.data.frame.default",
            "function(x, ...) {\n\
             if (is.atomic(x)) as.data.frame.vector(x, ...)\n\
             else stop(gettextf(\"cannot coerce class %s to a data.frame\",\n\
                                sQuote(deparse(class(x))[1L])), domain = NA)\n\
             }",
        );
        eval_base_binding(
            base_env,
            "as.data.frame.vector",
            include_str!("gnu_as_data_frame_vector.R"),
        );
        eval_base_binding(
            base_env,
            "as.data.frame.array",
            "function(x, ...) {\n\
             if (length(dim(x)) == 1L) {\n\
                 dim(x) <- NULL\n\
                 class(x) <- NULL\n\
                 as.data.frame.vector(x, ...)\n\
             } else as.data.frame.default(x, ...)\n\
             }",
        );

        eval_base_binding(base_env, "as.data.frame.raw", "as.data.frame.vector");
        eval_base_binding(base_env, "as.data.frame.factor", "as.data.frame.vector");
        eval_base_binding(base_env, "as.data.frame.ordered", "as.data.frame.vector");
        eval_base_binding(base_env, "as.data.frame.integer", "as.data.frame.vector");
        eval_base_binding(base_env, "as.data.frame.logical", "as.data.frame.vector");
        eval_base_binding(base_env, "as.data.frame.numeric", "as.data.frame.vector");
        eval_base_binding(base_env, "as.data.frame.complex", "as.data.frame.vector");
        eval_base_binding(base_env, "as.data.frame.Date", "as.data.frame.vector");
        eval_base_binding(base_env, "as.data.frame.POSIXct", "as.data.frame.vector");
        eval_base_binding(
            base_env,
            "as.data.frame.POSIXlt",
            include_str!("gnu_as_data_frame_POSIXlt.R"),
        );
        eval_base_binding(
            base_env,
            "as.data.frame.character",
            "function(x, ..., stringsAsFactors = FALSE) {\n\
             nm <- deparse1(substitute(x))\n\
             if (stringsAsFactors) x <- factor(x)\n\
             if (!\"nm\" %in% ...names())\n\
                 as.data.frame.vector(x, ..., nm = nm)\n\
             else as.data.frame.vector(x, ...)\n\
             }",
        );
        eval_base_binding(
            base_env,
            "as.data.frame.AsIs",
            "function(x, row.names = NULL, optional = FALSE, ...) {\n\
             d <- dim(x)\n\
             if (length(d) == 2L) {\n\
                 nrows <- d[1L]\n\
                 rn <- dimnames(x)[[1L]]\n\
             } else {\n\
                 nrows <- length(x)\n\
                 rn <- names(x)\n\
             }\n\
             if (is.null(row.names)) {\n\
                 if (!is.null(rn) && length(rn) == nrows && !anyDuplicated(rn))\n\
                     row.names <- rn\n\
                 else row.names <- if (nrows == 0L) character() else .set_row_names(nrows)\n\
             }\n\
             value <- list(x)\n\
             if (!optional) names(value) <- deparse(substitute(x), width.cutoff = 500L)[1L]\n\
             structure(value, row.names = row.names, class = \"data.frame\")\n\
             }",
        );
        eval_base_binding(
            base_env,
            "as.data.frame.model.matrix",
            "function(x, row.names = NULL, optional = FALSE, ...) {\n\
             d <- dim(x)\n\
             nrows <- d[1L]\n\
             rn <- dimnames(x)[[1L]]\n\
             if (is.null(row.names)) {\n\
                 if (!is.null(rn) && length(rn) == nrows && !anyDuplicated(rn))\n\
                     row.names <- rn\n\
                 else row.names <- if (nrows == 0L) character() else .set_row_names(nrows)\n\
             }\n\
             value <- list(x)\n\
             if (!optional) names(value) <- deparse(substitute(x), width.cutoff = 500L)[1L]\n\
             structure(value, row.names = row.names, class = \"data.frame\")\n\
             }",
        );
        eval_base_binding(
            base_env,
            "as.data.frame.data.frame",
            "function(x, row.names = NULL, ...) {\n\
             cl <- oldClass(x)\n\
             i <- match(\"data.frame\", cl)\n\
             if (i > 1L) class(x) <- cl[-(1L:(i - 1L))]\n\
             if (!is.null(row.names)) {\n\
                 nr <- .row_names_info(x, 2L)\n\
                 if (length(row.names) == nr) attr(x, \"row.names\") <- row.names\n\
                 else stop(\"invalid 'row.names' length\")\n\
             }\n\
             x\n\
             }",
        );
        eval_base_binding(
            base_env,
            ".rowNamesDF<-",
            include_str!("gnu_rowNamesDF.R"),
        );
        eval_base_binding(
            base_env,
            "as.data.frame.matrix",
            include_str!("gnu_as_data_frame_matrix.R"),
        );
        eval_base_binding(
            base_env,
            "as.data.frame.table",
            include_str!("gnu_as_data_frame_table.R"),
        );
        eval_base_binding(
            base_env,
            "as.data.frame.list",
            include_str!("gnu_as_data_frame_list.R"),
        );
        eval_base_binding(base_env, "data.frame", include_str!("gnu_data_frame.R"));
        eval_base_binding(
            base_env,
            "xtfrm.data.frame",
            "function(x) stop(\"cannot xtfrm data frames\")",
        );
        eval_base_binding(
            base_env,
            "remove.packages",
            "function(pkgs, lib) {\n\
             base <- pkgs %in% c(\"base\",\"compiler\",\"datasets\",\"graphics\",\"grDevices\",\"grid\",\"methods\",\"parallel\",\"splines\",\"stats\",\"stats4\",\"tcltk\",\"tools\",\"utils\")\n\
             if (any(base)) stop(paste0(\"package '\", pkgs[base][1], \"' is a base package, and cannot be removed\"), call. = FALSE)\n\
             invisible()\n\
             }",
        );
        eval_base_binding(
            base_env,
            "as.list.data.frame",
            "function(x, ...) { x <- unclass(x); attr(x, \"row.names\") <- NULL; x }",
        );
        eval_base_binding(
            base_env,
            "as.vector.data.frame",
            "function(x, mode = \"any\") { x <- as.list.data.frame(x); if (mode %in% c(\"any\", \"list\")) x else as.vector(x, mode = mode) }",
        );
        // GNU array.R / sapply.R: closures over .Internal, not primitives.
        // paste.R: .Internal(paste(list(...), sep, collapse, recycle0)).
        eval_base_binding(
            base_env,
            "paste",
            "function (..., sep = \" \", collapse = NULL, recycle0 = FALSE)\n\
             .Internal(paste(list(...), sep, collapse, recycle0))",
        );
        eval_base_binding(
            base_env,
            "paste0",
            "function (..., collapse = NULL, recycle0 = FALSE)\n\
             .Internal(paste0(list(...), collapse, recycle0))",
        );
        // GNU library.R / require.R: closures with substitute() NSE, not primitives.
        eval_base_binding(
            base_env,
            "library",
            "function(package, help, pos = 2, lib.loc = NULL, character.only = FALSE,\n\
             logical.return = FALSE, warn.conflicts, quietly = FALSE,\n\
             verbose = getOption(\"verbose\"), mask.ok, exclude, include.only,\n\
             attach.required = missing(include.only)) {\n\
             if (!missing(help)) stop(\"library help is not supported\")\n\
             if (!character.only) package <- as.character(substitute(package))\n\
             if (logical.return) return(invisible(.rport_require(package)))\n\
             .rport_library(package)\n\
             invisible(NULL)\n\
             }",
        );
        eval_base_binding(
            base_env,
            "require",
            "function(package, lib.loc = NULL, quietly = FALSE, warn.conflicts,\n\
             character.only = FALSE, mask.ok, exclude, include.only,\n\
             attach.required = missing(include.only)) {\n\
             if (!character.only) package <- as.character(substitute(package))\n\
             invisible(.rport_require(package))\n\
             }",
        );
        // GNU duplicated.R / factor.R: closures, not primitives.
        eval_base_binding(base_env, "is.factor", "function(x) inherits(x, \"factor\")");
        eval_base_binding(base_env, "is.ordered", "function(x) inherits(x, \"ordered\")");
        eval_base_binding(
            base_env,
            "unique",
            "function(x, incomparables = FALSE, ...) UseMethod(\"unique\")",
        );
        eval_base_binding(
            base_env,
            "unique.default",
            "function(x, incomparables = FALSE, fromLast = FALSE, nmax = NA, ...) {\n\
             if (!is.object(x))\n\
                 return(.Internal(unique(x, incomparables, fromLast, nmax)))\n\
             if (is.factor(x)) {\n\
                 z <- .Internal(unique(x, incomparables, fromLast,\n\
                                       min(length(x), nlevels(x) + 1L)))\n\
                 return(factor(z, levels = seq_len(nlevels(x)), labels = levels(x),\n\
                               ordered = is.ordered(x)))\n\
             }\n\
             .Internal(unique(x, incomparables, fromLast, nmax))\n\
             }",
        );
        eval_base_binding(
            base_env,
            "unique.data.frame",
            "function(x, incomparables = FALSE, fromLast = FALSE, ...) {\n\
             if (!isFALSE(incomparables))\n\
                 .NotYetUsed(\"incomparables != FALSE\")\n\
             x[!duplicated(x, fromLast = fromLast, ...), , drop = FALSE]\n\
             }",
        );
        eval_base_binding(
            base_env,
            "duplicated",
            "function(x, incomparables = FALSE, ...) UseMethod(\"duplicated\")",
        );
        eval_base_binding(
            base_env,
            "duplicated.default",
            "function(x, incomparables = FALSE, fromLast = FALSE, nmax = NA, ...) \n\
             .Internal(duplicated(x, incomparables, fromLast,\n\
                 if (is.factor(x)) min(length(x), nlevels(x) + 1L) else nmax))",
        );
        eval_base_binding(
            base_env,
            "anyDuplicated",
            "function(x, incomparables = FALSE, ...) UseMethod(\"anyDuplicated\")",
        );
        eval_base_binding(
            base_env,
            "anyDuplicated.default",
            "function(x, incomparables = FALSE, fromLast = FALSE, ...)\n\
             .Internal(anyDuplicated(x, incomparables, fromLast))",
        );
        eval_base_binding(
            base_env,
            "deparse1",
            "function(expr, collapse = \" \", width.cutoff = 500L, ...)\n\
             paste(deparse(expr, width.cutoff, ...), collapse = collapse)",
        );
        eval_base_binding(
            base_env,
            "detach",
            "function(name, pos = 2L, unload = FALSE, character.only = FALSE, force = FALSE) {\n\
             if (!missing(name)) {\n\
                 if (!character.only) name <- substitute(name)\n\
                 pos <- if (is.numeric(name)) name else {\n\
                     if (!is.character(name)) name <- deparse1(name)\n\
                     match(name, search())\n\
                 }\n\
                 if (is.na(pos)) stop(\"invalid 'name' argument\")\n\
             }\n\
             invisible(.Internal(detach(pos)))\n\
             }",
        );
        eval_base_binding(
            base_env,
            "data",
            "function(..., list = character(), package = NULL, lib.loc = NULL,\n\
             verbose = getOption(\"verbose\"), envir = .GlobalEnv, overwrite = TRUE) {\n\
             dots <- as.character(substitute(list(...)))[-1L]\n\
             if (length(list)) dots <- c(dots, list)\n\
             .Internal(data(dots, package, envir))\n\
             }",
        );
        eval_base_binding(base_env, "levels", "function(x) UseMethod(\"levels\")");
        eval_base_binding(base_env, "levels.default", "function(x) attr(x, \"levels\")");
        eval_base_binding(base_env, "nlevels", "function(x) length(levels(x))");
        eval_base_binding(
            base_env,
            "droplevels",
            "function(x, ...) UseMethod(\"droplevels\")",
        );
        eval_base_binding(
            base_env,
            "droplevels.factor",
            "function(x, exclude = if (anyNA(levels(x))) NULL else NA, ...)\n\
             factor(x, exclude = exclude)",
        );
        eval_base_binding(
            base_env,
            "droplevels.data.frame",
            "function(x, except = NULL, exclude, ...) {\n\
             ix <- vapply(x, is.factor, NA)\n\
             if (!is.null(except)) ix[except] <- FALSE\n\
             x[ix] <- if (missing(exclude)) lapply(x[ix], droplevels)\n\
                      else lapply(x[ix], droplevels, exclude = exclude)\n\
             x\n\
             }",
        );
        eval_base_binding(
            base_env,
            "ordered",
            "function(x = character(), ...) factor(x, ..., ordered = TRUE)",
        );
        eval_base_binding(
            base_env,
            "as.ordered",
            "function(x) if (is.ordered(x)) x else ordered(x)",
        );
        eval_base_binding(
            base_env,
            "attach",
            "function(what, pos = 2L, name = deparse1(substitute(what)),\n\
             warn.conflicts = TRUE) {\n\
             if (pos == 1L)\n\
                 stop(\"'pos=1' is not possible and has been warned about for years\")\n\
             invisible(.Internal(attach(what, pos, name)))\n\
             }",
        );
        eval_base_binding(base_env, "nrow", "function(x) dim(x)[1L]");
        eval_base_binding(base_env, "ncol", "function(x) dim(x)[2L]");
        eval_base_binding(
            base_env,
            "NROW",
            "function(x) if (length(d <- dim(x))) d[1L] else length(x)",
        );
        eval_base_binding(
            base_env,
            "NCOL",
            "function(x) if (is.null(x)) 0L else if (length(d <- dim(x)) > 1L) d[2L] else 1L",
        );
        eval_base_binding(
            base_env,
            "%in%",
            "function(x, table) match(x, table, nomatch = 0L) > 0L",
        );
        eval_base_binding(
            base_env,
            "isTRUE",
            "function(x) is.logical(x) && length(x) == 1L && !is.na(x) && x",
        );
        eval_base_binding(
            base_env,
            "isFALSE",
            "function(x) is.logical(x) && length(x) == 1L && !is.na(x) && !x",
        );
        eval_base_binding(base_env, "force", "function(x) x");
        // GNU eval.R / which.R / stop.R: these are closures over .Internal,
        // not FunTab primitives. Binding them into the base frame makes
        // exists()/as.list(baseenv()) match GNU; .Internal still dispatches
        // through FunTab (eval=11/211). parent.frame must be a wrapper so
        // do_parentframe walks from that extra frame (GNU context.c).
        eval_base_binding(base_env, "parent.frame", include_str!("gnu_parent_frame.R"));
        eval_base_binding(
            base_env,
            "sys.call",
            "function(which = 0L) .Internal(sys.call(which))",
        );
        eval_base_binding(base_env, "sys.calls", "function() .Internal(sys.calls())");
        eval_base_binding(
            base_env,
            "sys.frame",
            "function(which = 0L) .Internal(sys.frame(which))",
        );
        eval_base_binding(
            base_env,
            "sys.function",
            "function(which = 0L) .Internal(sys.function(which))",
        );
        eval_base_binding(base_env, "sys.frames", "function() .Internal(sys.frames())");
        eval_base_binding(base_env, "sys.nframe", "function() .Internal(sys.nframe())");
        eval_base_binding(
            base_env,
            "sys.parent",
            "function(n = 1L) .Internal(sys.parent(n))",
        );
        eval_base_binding(base_env, "sys.parents", "function() .Internal(sys.parents())");
        eval_base_binding(base_env, "sys.on.exit", "function() .Internal(sys.on.exit())");
        eval_base_binding(
            base_env,
            "sys.status",
            "function() list(sys.calls = sys.calls(), sys.parents = sys.parents(), sys.frames = sys.frames())",
        );

        eval_base_binding(base_env, "eval", include_str!("gnu_eval.R"));
        eval_base_binding(base_env, "evalq", include_str!("gnu_evalq.R"));
        eval_base_binding(base_env, "eval.parent", include_str!("gnu_eval_parent.R"));
        eval_base_binding(base_env, "arrayInd", include_str!("gnu_arrayInd.R"));
        eval_base_binding(base_env, "which", include_str!("gnu_which.R"));
        eval_base_binding(base_env, "which.min", include_str!("gnu_which_min.R"));
        eval_base_binding(base_env, "which.max", include_str!("gnu_which_max.R"));
        eval_base_binding(base_env, "stopifnot", include_str!("gnu_stopifnot.R"));
        eval_base_binding(
            base_env,
            "getElement",
            "function(object, name) if(isS4(object)) methods::slot(object, name) else object[[name, exact=TRUE]]",
        );
        eval_base_binding(
            base_env,
            "write",
            "function(x, file = \"data\", ncolumns = if(is.character(x)) 1 else 5, append = FALSE, sep = \" \") cat(x, file = file, sep = c(rep.int(sep, ncolumns-1), \"\\n\"), append = append)",
        );
        eval_base_binding(base_env, "strptime", include_str!("gnu_strptime.R"));

        eval_base_binding(
            base_env,
            "is.primitive",
            "function(x) switch(typeof(x), special = , builtin = TRUE, FALSE)",
        );
        eval_base_binding(
            base_env,
            "as.symbol",
            "function(x) .Internal(as.vector(x, \"symbol\"))",
        );
        eval_base_binding(base_env, "as.name", "as.symbol");
        eval_base_binding(
            base_env,
            "unname",
            "function(obj, force = FALSE) {\n\
             if (!is.null(names(obj))) names(obj) <- NULL\n\
             if (!is.null(dimnames(obj)) && (force || !is.data.frame(obj)))\n\
                 dimnames(obj) <- NULL\n\
             obj\n\
             }",
        );
        eval_base_binding(
            base_env,
            "%||%",
            "function(x, y) if (is.null(x)) y else x",
        );
        eval_base_binding(
            base_env,
            "structure",
            "function(.Data, ...) {\n\
             if (is.null(.Data)) stop(\"attempt to set an attribute on NULL\")\n\
             attrib <- list(...)\n\
             if (length(attrib)) {\n\
                 specials <- c(\".Dim\", \".Dimnames\", \".Names\", \".Tsp\", \".Label\")\n\
                 attrnames <- names(attrib)\n\
                 m <- match(attrnames, specials)\n\
                 ok <- !is.na(m)\n\
                 if (any(ok)) {\n\
                     replace <- c(\"dim\", \"dimnames\", \"names\", \"tsp\", \"levels\")\n\
                     names(attrib)[ok] <- replace[m[ok]]\n\
                 }\n\
                 if (isTRUE(any(attrib[[\"class\", exact = TRUE]] == \"factor\"))\n\
                     && typeof(.Data) == \"double\")\n\
                     storage.mode(.Data) <- \"integer\"\n\
                 attributes(.Data) <- c(attributes(.Data), attrib)\n\
             }\n\
             .Data\n\
             }",
        );
        eval_base_binding(base_env, "mostattributes<-", include_str!("gnu_mostattributes.R"));
        eval_base_binding(base_env, "format.pval", include_str!("gnu_format_pval.R"));
        eval_base_binding(base_env, "format", "function(x, ...) UseMethod(\"format\")");
        eval_base_binding(
            base_env,
            "addTaskCallback",
            "function(f, data = NULL, name = character()) {\n\
                if (!is.function(f)) stop(\"handler must be a function\")\n\
                .Internal(addTaskCallback(f, data))\n\
            }",
        );
        eval_base_binding(
            base_env,
            "removeTaskCallback",
            "function(id) .Internal(removeTaskCallback(id))",
        );
        eval_base_binding(
            base_env,
            "Math.data.frame",
            "function(x, ...) {\n\
                mode.ok <- vapply(x, function(x) is.numeric(x) || is.logical(x) || is.complex(x), NA)\n\
                if (all(mode.ok)) {\n\
                    x[] <- lapply(X = x, FUN = .Generic, ...)\n\
                    x\n\
                } else {\n\
                    vnames <- names(x)\n\
                    if (is.null(vnames)) vnames <- seq_along(x)\n\
                    stop(\"non-numeric-alike variable(s) in data frame: \", paste(vnames[!mode.ok], collapse = \", \"))\n\
                }\n\
            }",
        );
        eval_base_binding(
            base_env,
            "Summary.data.frame",
            "function(..., na.rm = FALSE) {\n\
                args <- list(...)\n\
                args <- lapply(args, function(x) {\n\
                    x <- as.matrix(x)\n\
                    if (!is.numeric(x) && !is.logical(x) && !is.complex(x))\n\
                        stop(\"only defined on a data frame with all numeric-alike variables\")\n\
                    x\n\
                })\n\
                do.call(.Generic, c(args, na.rm = na.rm))\n\
            }",
        );
        eval_base_binding(
            base_env,
            "diag<-",
            "function(x, value) {\n\
                dx <- dim(x)\n\
                if (length(dx) != 2L)\n\
                    stop(\"only matrix diagonals can be replaced\")\n\
                len.i <- min(dx)\n\
                len.v <- length(value)\n\
                if (len.v != 1L && len.v != len.i)\n\
                    stop(\"replacement diagonal has wrong length\")\n\
                if (len.i) {\n\
                    i <- seq_len(len.i)\n\
                    x[cbind(i, i)] <- value\n\
                }\n\
                x\n\
            }",
        );
        eval_base_binding(
            base_env,
            "diff",
            "function(x, ...) UseMethod(\"diff\")",
        );
        eval_base_binding(
            base_env,
            "diff.default",
            include_str!("gnu_diff_default.R"),
        );
        eval_base_binding(
            base_env,
            "curve",
            include_str!("gnu_curve.R"),
        );
        eval_base_binding(
            base_env,
            "proportions",
            include_str!("gnu_proportions.R"),
        );
        eval_base_binding(base_env, "prop.table", "proportions");
        eval_base_binding(base_env, "pdf", "function(...) invisible(NULL)");
        eval_base_binding(base_env, "dev.off", "function(...) 1L");
        eval_base_binding(base_env, "postscript", "function(...) invisible(NULL)");
        eval_base_binding(base_env, "legend", "function(...) invisible(NULL)");
        eval_base_binding(base_env, "dev.hold", "function(...) invisible(NULL)");
        eval_base_binding(base_env, "dev.flush", "function(...) invisible(NULL)");
        eval_base_binding(base_env, "frame", "function(...) invisible(NULL)");
        eval_base_binding(base_env, "as.null", "function(x) NULL");
        eval_base_binding(base_env, "mtext", "function(...) invisible(NULL)");
        eval_base_binding(base_env, "duplicated", include_str!("gnu_duplicated.R"));
        eval_base_binding(base_env, "duplicated.default", include_str!("gnu_duplicated_default.R"));
        eval_base_binding(
            base_env,
            "duplicated.data.frame",
            include_str!("gnu_duplicated_data_frame.R"),
        );
        eval_base_binding(
            base_env,
            "anyDuplicated.data.frame",
            "function(x, incomparables = FALSE, fromLast = FALSE, ...) {\n\
             if (!isFALSE(incomparables)) .NotYetUsed(\"incomparables != FALSE\")\n\
             if (any(i <- (lengths(lapply(x, dim)) == 2L)))\n\
                 x[i] <- lapply(x[i], split.data.frame, seq_len(nrow(x)))\n\
             anyDuplicated(do.call(Map, `names<-`(c(list, x), NULL)), fromLast = fromLast)\n\
             }",
        );
        eval_base_binding(
            base_env,
            "Map",
            "function(f, ...) { f <- match.fun(f); mapply(FUN = f, ..., SIMPLIFY = FALSE) }",
        );
        eval_base_binding(base_env, "xy.coords", include_str!("gnu_xy_coords.R"));
        eval_base_binding(base_env, "fix", include_str!("gnu_fix.R"));
        eval_base_binding(base_env, "edit", "function(name, ...) name");
        eval_base_binding(base_env, "image", "function(...) invisible(NULL)");
        eval_base_binding(
            base_env,
            "contour",
            "function(x, ...) { lv <- list(...)$levels; if (!is.null(lv)) { bad <- which(!is.finite(lv)); if (length(bad)) stop(sprintf('non-finite level values: levels[%d] = %g', bad[1L], lv[bad[1L]])) }; invisible(NULL) }",
        );
        eval_base_binding(base_env, "apropos", include_str!("gnu_apropos.R"));
        eval_base_binding(base_env, "persp", "function(...) invisible(NULL)");
        eval_base_binding(base_env, "heat.colors", "function(n, ...) rep(\"#FF0000\", n)");
        eval_base_binding(base_env, "rainbow", "function(n, ...) rep(\"#FF0000\", n)");
        eval_base_binding(base_env, "plot.formula", include_str!("gnu_plot_formula.R"));
        eval_base_binding(base_env, "plot.data.frame", "function(x, ...) invisible(NULL)");
        eval_base_binding(base_env, "colorRamp", include_str!("gnu_color_ramp.R"));
        eval_base_binding(
            base_env,
            "colorRampPalette",
            include_str!("gnu_color_ramp_palette.R"),
        );
        eval_base_binding(base_env, "subset", "function(x, ...) UseMethod(\"subset\")");
        eval_base_binding(
            base_env,
            "subset.default",
            "function(x, subset, ...) {\n    if(!is.logical(subset)) stop(\"'subset' must be logical\")\n    x[subset & !is.na(subset)]\n}",
        );
        eval_base_binding(
            base_env,
            "subset.data.frame",
            include_str!("gnu_subset_data_frame.R"),
        );
        eval_base_binding(base_env, "relist", include_str!("gnu_relist.R"));
        eval_base_binding(base_env, "relist.default", include_str!("gnu_relist_default.R"));
        eval_base_binding(base_env, "relist.list", include_str!("gnu_relist_list.R"));
        eval_base_binding(base_env, "outer", include_str!("gnu_outer.R"));
        eval_base_binding(base_env, "%o%", "function(X, Y) outer(X, Y)");
        eval_base_binding(
            base_env,
            "str2lang",
            "function(s) { stopifnot(length(s) == 1L); ex <- parse(text = s, keep.source = FALSE); stopifnot(length(ex) == 1L); ex[[1L]] }",
        );
        eval_base_binding(
            base_env,
            "str2expression",
            "function(text) parse(text = text, keep.source = FALSE)",
        );
        eval_base_binding(base_env, "getAnywhere", include_str!("gnu_getAnywhere.R"));
        eval_base_binding(base_env, "prompt", include_str!("gnu_prompt.R"));
        eval_base_binding(base_env, "prompt.default", include_str!("gnu_prompt_default.R"));
        eval_base_binding(
            base_env,
            "deparse1",
            "function(expr, collapse = \" \", width.cutoff = 500L, ...) paste(deparse(expr, width.cutoff, ...), collapse = collapse)",
        );
        eval_base_binding(base_env, "srcfile", include_str!("gnu_srcfile.R"));
        eval_base_binding(base_env, "R.home", include_str!("gnu_r_home.R"));
        eval_base_binding(base_env, "as.expression", "function(x, ...) UseMethod(\"as.expression\")");
        eval_base_binding(
            base_env,
            "as.expression.default",
            "function(x, ...) .Internal(as.vector(x, \"expression\"))",
        );
        eval_base_binding(base_env, "labels", "function(object, ...) UseMethod(\"labels\")");
        eval_base_binding(
            base_env,
            "labels.default",
            "function(object, ...) {\n    if(length(d <- dim(object))) {\n        nt <- dimnames(object)\n        if(is.null(nt)) nt <- vector(\"list\", length(d))\n        for(i in seq_along(d))\n            if(!length(nt[[i]])) nt[[i]] <- as.character(seq_len(d[i]))\n    } else {\n        nt <- names(object)\n        if(!length(nt)) nt <- as.character(seq_along(object))\n    }\n    nt\n}",
        );
        eval_base_binding(
            base_env,
            "rapply",
            "function(object, f, classes = \"ANY\", deflt = NULL, how = c(\"unlist\", \"replace\", \"list\"), ...) {\n\
             how <- match.arg(how)\n\
             res <- .Internal(rapply(object, f, classes, deflt, how))\n\
             if (how == \"unlist\") unlist(res, recursive = TRUE) else res\n\
             }",
        );
        eval_base_binding(base_env, "within", "function(data, expr, ...) UseMethod(\"within\")");
        eval_base_binding(
            base_env,
            "within.data.frame",
            include_str!("gnu_within_data_frame.R"),
        );
        eval_base_binding(base_env, "within.list", include_str!("gnu_within_list.R"));
        eval_base_binding(
            base_env,
            "rbind.data.frame",
            include_str!("gnu_rbind_data_frame.R"),
        );
        eval_base_binding(base_env, ".checkHT", include_str!("gnu_checkHT.R"));
        eval_base_binding(base_env, "head", "function(x, ...) UseMethod(\"head\")");
        eval_base_binding(base_env, "head.default", include_str!("gnu_head_default.R"));
        eval_base_binding(base_env, "head.array", include_str!("gnu_head_array.R"));
        eval_base_binding(base_env, "head.matrix", include_str!("gnu_head_array.R"));
        eval_base_binding(base_env, "tail", "function(x, ...) UseMethod(\"tail\")");
        eval_base_binding(base_env, "tail.default", include_str!("gnu_tail_default.R"));
        eval_base_binding(base_env, "tail.array", include_str!("gnu_tail_array.R"));
        eval_base_binding(base_env, "tail.matrix", include_str!("gnu_tail_array.R"));
        eval_base_binding(
            base_env,
            "labels.dendrogram",
            "function(object, ...) {\n    if(is.list(object))\n        rapply(object, attr, which = \"label\")\n    else\n        attr(object, \"label\")\n}",
        );
        eval_base_binding(
            base_env,
            "dendrapply",
            "function(X, FUN, ...) {\n    FUN <- match.fun(FUN)\n    if (!inherits(X, \"dendrogram\")) stop(\"'X' is not a dendrogram\")\n    Napply <- function(d) {\n        r <- FUN(d, ...)\n        if (!is.leaf(d)) {\n            if (!is.list(r)) r <- as.list(r)\n            if (length(r) < (n <- length(d))) r[seq_len(n)] <- vector(\"list\", n)\n            r[] <- lapply(d, Napply)\n        }\n        r\n    }\n    Napply(X)\n}",
        );
        eval_base_binding(base_env, "as.dendrogram", include_str!("gnu_dendrogram.R"));
        eval_base_binding(
            base_env,
            "reorder",
            "function(x, ...) if (inherits(x, \"dendrogram\")) .rport_reorder_dendrogram(x, ..1) else .rport_reorder_default(x, ..1)",
        );
        eval_base_binding(base_env, "as.hclust", "function(x, ...) UseMethod(\"as.hclust\")");
        eval_base_binding(
            base_env,
            "as.hclust.default",
            "function(x, ...) {\n\
             if (inherits(x, \"hclust\")) x else\n\
             stop(gettextf(\"argument 'x' cannot be coerced to class %s\", dQuote(\"hclust\")), domain = NA)\n\
             }",
        );
        eval_base_binding(base_env, "tar", include_str!("gnu_tar.R"));
        eval_base_binding(base_env, "kronecker", include_str!("gnu_kronecker.R"));
        eval_base_binding(
            base_env,
            "ps.options",
            "function(...) list(onefile = TRUE)",
        );
        eval_base_binding(base_env, "match.fun", include_str!("gnu_match_fun.R"));
        eval_base_binding(base_env, "summaryRprof", include_str!("gnu_summary_rprof.R"));
        eval_base_binding(base_env, "subset.matrix", include_str!("gnu_subset_matrix.R"));
        eval_base_binding(base_env, "kappa", include_str!("gnu_kappa.R"));
        eval_base_binding(base_env, "kappa.lm", include_str!("gnu_kappa_lm.R"));
        eval_base_binding(base_env, "kappa.qr", include_str!("gnu_kappa_qr.R"));
        eval_base_binding(base_env, ".kappa_tri", include_str!("gnu_kappa_tri.R"));
        eval_base_binding(base_env, "merge", include_str!("gnu_merge.R"));
        eval_base_binding(base_env, "merge.default", include_str!("gnu_merge_default.R"));
        eval_base_binding(
            base_env,
            "merge.data.frame",
            include_str!("gnu_merge_data_frame.R"),
        );
        eval_base_binding(
            base_env,
            "maintainer",
            "function(pkg) { force(pkg); desc <- try(packageDescription(pkg), silent = TRUE); if (is.list(desc)) gsub(\"\\n\", \" \", desc$Maintainer, fixed = TRUE) else NA_character_ }",
        );
        eval_base_binding(
            base_env,
            "graphics.off",
            "function() invisible()",
        );
        eval_base_binding(
            base_env,
            "dev.interactive",
            "function(orNone = FALSE) FALSE",
        );
        eval_base_binding(
            base_env,
            "callCC",
            "function(fun) { value <- NULL; delayedAssign(\"throw\", return(value)); fun(function(v) { value <<- v; throw }) }",
        );
        eval_base_binding(
            base_env,
            "delayedAssign",
            "function(x, value, eval.env = parent.frame(1), assign.env = parent.frame(1)) .Internal(delayedAssign(x, substitute(value), eval.env, assign.env))",
        );
        eval_base_binding(
            base_env,
            "all.equal",
            "function(target, current, ...) UseMethod(\"all.equal\")",
        );
        eval_base_binding(base_env, "attr.all.equal", include_str!("gnu_attr_all_equal.R"));
        eval_base_binding(
            base_env,
            "count.fields",
            "function(file, sep = \"\", quote = \"\\\"'\", skip = 0,\n         blank.lines.skip = TRUE, comment.char = \"#\")\n{\n    if(is.character(file)) {\n        file <- file(file)\n        on.exit(close(file))\n    }\n    if(!inherits(file, \"connection\"))\n        stop(\"'file' must be a character string or connection\")\n    if (!isOpen(file)) open(file, \"rt\")\n    .External(C_countfields, file, sep, quote, skip, blank.lines.skip,\n              comment.char)\n}\n",
        );
        eval_base_binding(base_env, "C_countfields", "\"C_countfields\"");
        eval_base_binding(base_env, "C_runmed", "\"C_runmed\"");
        eval_base_binding(
            base_env,
            "suppressPackageStartupMessages",
            "function (expr) withCallingHandlers(expr, packageStartupMessage = function(c) tryInvokeRestart(\"muffleMessage\"))",
        );
        eval_base_binding(base_env, "sub", include_str!("gnu_sub.R"));
        eval_base_binding(base_env, "gsub", include_str!("gnu_gsub.R"));
        eval_base_binding(base_env, "grep", include_str!("gnu_grep.R"));
        eval_base_binding(base_env, "grepl", include_str!("gnu_grepl.R"));
        eval_base_binding(base_env, "regexpr", include_str!("gnu_regexpr.R"));
        eval_base_binding(base_env, "gregexpr", include_str!("gnu_gregexpr.R"));
        eval_base_binding(
            base_env,
            "summary.connection",
            "function(object, ...) .Internal(summary.connection(object))",
        );
        eval_base_binding(base_env, "srcfilecopy", include_str!("gnu_srcfilecopy.R"));

    eval_base_binding(base_env, ".traceback", include_str!("gnu_dot_traceback.R"));
    eval_base_binding(base_env, "traceback", include_str!("gnu_traceback.R"));
    eval_base_binding(base_env, "get_all_vars", include_str!("gnu_get_all_vars.R"));
        eval_base_binding(base_env, "poly", include_str!("gnu_poly.R"));
        eval_base_binding(base_env, "polym", include_str!("gnu_polym.R"));
        eval_base_binding(base_env, "predict.poly", include_str!("gnu_predict_poly.R"));
        eval_base_binding(
            base_env,
            "makepredictcall.poly",
            include_str!("gnu_makepredictcall_poly.R"),
        );
        eval_base_binding(
            base_env,
            "all.equal.character",
            include_str!("gnu_all_equal_character.R"),
        );
        eval_base_binding(
            base_env,
            "all.equal.numeric",
            include_str!("gnu_all_equal_numeric.R"),
        );
        eval_base_binding(
            base_env,
            "all.equal.list",
            include_str!("gnu_all_equal_list.R"),
        );
        eval_base_binding(
            base_env,
            "all.equal.default",
            include_str!("gnu_all_equal_default.R"),
        );
        eval_base_binding(
            base_env,
            "all.equal.language",
            include_str!("gnu_all_equal_language.R"),
        );
        eval_base_binding(
            base_env,
            "all.equal.function",
            include_str!("gnu_all_equal_function.R"),
        );
        eval_base_binding(
            base_env,
            "all.equal.environment",
            include_str!("gnu_all_equal_environment.R"),
        );
        eval_base_binding(
            base_env,
            "all.equal.formula",
            include_str!("gnu_all_equal_formula.R"),
        );
        eval_base_binding(
            base_env,
            "all.equal.raw",
            include_str!("gnu_all_equal_raw.R"),
        );
        eval_base_binding(base_env, "all.equal.logical", "all.equal.raw");
        eval_base_binding(
            base_env,
            "all.equal.factor",
            include_str!("gnu_all_equal_factor.R"),
        );
        eval_base_binding(base_env, "all.equal.integer", "all.equal.numeric");
        eval_base_binding(base_env, "all.equal.complex", "all.equal.numeric");
        eval_base_binding(
            base_env,
            "data.class",
            "function(x) {\n\
             if (length(cl <- oldClass(x))) cl[1L] else mode(x)\n\
             }",
        );
        eval_base_binding(
            base_env,
            "is.qr",
            "function(x) is.list(x) && inherits(x, \"qr\")",
        );
        eval_base_binding(
            base_env,
            "qr.solve",
            "function(a, b, tol = 1e-7) {\n\
             if (!inherits(a, \"qr\"))\n\
                 a <- qr(a, tol = tol)\n\
             nc <- ncol(a$qr); nr <- nrow(a$qr)\n\
             if (a$rank != min(nc, nr))\n\
                 stop(\"singular matrix 'a' in solve\")\n\
             if (missing(b)) {\n\
                 if (nc != nr)\n\
                     stop(\"only square matrices can be inverted\")\n\
                 b <- diag(1, nc)\n\
             }\n\
             res <- qr.coef(a, b)\n\
             res[is.na(res)] <- 0\n\
             res\n\
             }",
        );
        eval_base_binding(
            base_env,
            "rownames",
            "function(x, do.NULL = TRUE, prefix = \"row\") {\n\
             dn <- dimnames(x)[[1L]]\n\
             if (!is.null(dn)) dn else if (do.NULL) NULL else {\n\
                 nr <- NROW(x)\n\
                 if (nr > 0L) paste0(prefix, seq_len(nr)) else character()\n\
             }\n\
             }",
        );
        eval_base_binding(
            base_env,
            "colnames",
            "function(x, do.NULL = TRUE, prefix = \"col\") {\n\
             if (is.data.frame(x) && do.NULL) names(x) else {\n\
                 dn <- dimnames(x)[[2L]]\n\
                 if (!is.null(dn)) dn else if (do.NULL) NULL else {\n\
                     nc <- NCOL(x)\n\
                     if (nc > 0L) paste0(prefix, seq_len(nc)) else character()\n\
                 }\n\
             }\n\
             }",
        );
        eval_base_binding(
            base_env,
            "rownames<-",
            "function(x, value) {\n\
             if (is.data.frame(x)) {\n\
                 x <- `row.names<-.data.frame`(x, value)\n\
             } else {\n\
                 dn <- dimnames(x)\n\
                 if (is.null(dn)) {\n\
                     if (is.null(value)) return(x)\n\
                     if ((nd <- length(dim(x))) < 1L)\n\
                         stop(\"attempt to set 'rownames' on an object with no dimensions\")\n\
                     dn <- vector(\"list\", nd)\n\
                 }\n\
                 if (length(dn) < 1L)\n\
                     stop(\"attempt to set 'rownames' on an object with no dimensions\")\n\
                 if (is.null(value)) dn[1L] <- list(NULL) else dn[[1L]] <- value\n\
                 dimnames(x) <- dn\n\
             }\n\
             x\n\
             }",
        );
        eval_base_binding(
            base_env,
            "colnames<-",
            "function(x, value) {\n\
             if (is.data.frame(x)) {\n\
                 names(x) <- value\n\
             } else {\n\
                 dn <- dimnames(x)\n\
                 if (is.null(dn)) {\n\
                     if (is.null(value)) return(x)\n\
                     if ((nd <- length(dim(x))) < 2L)\n\
                         stop(\"attempt to set 'colnames' on an object with less than two dimensions\")\n\
                     dn <- vector(\"list\", nd)\n\
                 }\n\
                 if (length(dn) < 2L)\n\
                     stop(\"attempt to set 'colnames' on an object with less than two dimensions\")\n\
                 if (is.null(value)) dn[2L] <- list(NULL) else dn[[2L]] <- value\n\
                 dimnames(x) <- dn\n\
             }\n\
             x\n\
             }",
        );
        eval_base_binding(base_env, "row.names", "function(x) UseMethod(\"row.names\")");
        eval_base_binding(
            base_env,
            "row.names.data.frame",
            "function(x) as.character(attr(x, \"row.names\"))",
        );
        eval_base_binding(
            base_env,
            "row.names.default",
            "function(x) if (!is.null(dim(x))) rownames(x)",
        );
        eval_base_binding(
            base_env,
            ".row",
            "function(dim) row(matrix(0, dim[1L], dim[2L]))",
        );
        eval_base_binding(
            base_env,
            ".col",
            "function(dim) col(matrix(0, dim[1L], dim[2L]))",
        );
        eval_base_binding(
            base_env,
            "row.names<-",
            "function(x, value) UseMethod(\"row.names<-\")",
        );
        eval_base_binding(
            base_env,
            "row.names<-.default",
            "function(x, value) `rownames<-`(x, value)",
        );
        eval_base_binding(
            base_env,
            "row.names<-.data.frame",
            "function(x, value) `.rowNamesDF<-`(x, value = value)",
        );
        eval_base_binding(base_env, "seq", "function(...) UseMethod(\"seq\")");
        eval_base_binding(base_env, "chkDots", include_str!("gnu_chkDots.R"));
        eval_base_binding(base_env, "seq.default", include_str!("gnu_seq_default.R"));
        eval_base_binding(base_env, "units", "function(x) UseMethod(\"units\")");
        eval_base_binding(base_env, "units<-", "function(x, value) UseMethod(\"units<-\")");
        eval_base_binding(
            base_env,
            "substring<-",
            "function(text, first, last = NULL, value) `substr<-`(text, first, last, value)",
        );
        eval_base_binding(base_env, "units.difftime", "function(x) attr(x, \"units\")");
        eval_base_binding(
            base_env,
            "summary.POSIXct",
            "function(object, digits = 15L, ...) {\n\
             x <- summary.default(unclass(object), digits = digits, ...)\n\
             nas <- NULL\n\
             if (m <- match(\"NAs\", names(x), 0L)) {\n\
               nas <- as.integer(x[m])\n\
               x <- x[-m]\n\
               attr(x, \"NAs\") <- nas\n\
             }\n\
             class(x) <- c(\"summaryDefault\", oldClass(object))\n\
             x\n\
            }",
        );
        eval_base_binding(
            base_env,
            "summary.POSIXlt",
            "function(object, ...) summary(as.POSIXct(object), ...)",
        );
        eval_base_binding(
            base_env,
            "units<-.difftime",
            include_str!("gnu_units_difftime.R"),
        );
        eval_base_binding(
            base_env,
            "[<-.difftime",
            include_str!("gnu_subset_assign_difftime.R"),
        );
        eval_base_binding(
            base_env,
            "as.double.difftime",
            "function(x, units = \"auto\", ...) {\n\
                 if (units != \"auto\") units(x) <- units\n\
                 as.vector(x, \"double\")\n\
             }",
        );
        eval_base_binding(
            base_env,
            "as.numeric.difftime",
            "function(x, units = \"auto\", ...) {\n\
                 if (units != \"auto\") units(x) <- units\n\
                 as.vector(x, \"double\")\n\
             }",
        );
        eval_base_binding(base_env, "trunc.POSIXt", include_str!("gnu_trunc_POSIXt.R"));
        eval_base_binding(base_env, "seq.POSIXt", include_str!("gnu_seq_POSIXt.R"));
        eval_base_binding(base_env, "seq.Date", include_str!("gnu_seq_Date.R"));
        eval_base_binding(base_env, "pretty.POSIXt", include_str!("gnu_pretty_date.R"));
        eval_base_binding(base_env, "Reduce", include_str!("gnu_reduce.R"));
        eval_base_binding(base_env, "axTicks", include_str!("gnu_axTicks.R"));
        eval_base_binding(base_env, "axisTicks", include_str!("gnu_axis_ticks.R"));
        eval_base_binding(base_env, "bxp", include_str!("gnu_bxp.R"));
        eval_base_binding(base_env, "reformulate", include_str!("gnu_reformulate.R"));
        eval_base_binding(base_env, ".Deprecated", include_str!("gnu_deprecated.R"));
        eval_base_binding(base_env, "simpleMessage", include_str!("gnu_simple_message.R"));
        eval_base_binding(base_env, "getHook", include_str!("gnu_userhooks.R"));
        eval_base_binding(base_env, "grepRaw", include_str!("gnu_grepRaw.R"));
        eval_base_binding(base_env, "symnum", include_str!("gnu_symnum.R"));
        eval_base_binding(base_env, "write.dcf", include_str!("gnu_write_dcf.R"));
        eval_base_binding(
            base_env,
            "c.noquote",
            "function(..., recursive = FALSE) structure(NextMethod(\"c\"), class = \"noquote\")",
        );
        eval_base_binding(
            base_env,
            "print.noquote",
            include_str!("gnu_print_noquote.R"),
        );
        eval_base_binding(base_env, "ls", include_str!("gnu_ls.R"));
        eval_base_binding(base_env, "objects", include_str!("gnu_ls.R"));
        eval_base_binding(
            base_env,
            "as.vector.factor",
            include_str!("gnu_as_vector_factor.R"),
        );
        eval_base_binding(
            base_env,
            "as.list.factor",
            "function(x, ...) { res <- vector(\"list\", length(x)); for (i in seq_along(x)) res[[i]] <- x[[i]]; if (is.null(names(x))) res else `names<-`(res, names(x)) }",
        );
        eval_base_binding(
            base_env,
            "as.POSIXlt.POSIXct",
            include_str!("gnu_as_POSIXlt_POSIXct.R"),
        );

        eval_base_binding(
            base_env,
            "as.Date.default",
            "function(x, ...) { if (inherits(x, \"Date\")) x else if (is.null(x)) structure(numeric(), class = \"Date\") else if (is.logical(x) && all(is.na(x))) structure(as.numeric(x), class = \"Date\") else stop(gettextf(\"do not know how to convert '%s' to class %s\", deparse1(substitute(x)), dQuote(\"Date\")), domain = NA) }",
        );
        eval_base_binding(base_env, "print.Date", include_str!("gnu_print_Date.R"));
        eval_base_binding(
            base_env,
            "length<-.POSIXct",
            include_str!("gnu_lengthgets_POSIXct.R"),
        );
        eval_base_binding(
            base_env,
            "length<-.POSIXlt",
            include_str!("gnu_lengthgets_POSIXlt.R"),
        );
        eval_base_binding(base_env, "print.POSIXct", include_str!("gnu_print_POSIXt.R"));
        eval_base_binding(base_env, "print.POSIXlt", include_str!("gnu_print_POSIXt.R"));
        eval_base_binding(
            base_env,
            ".valid.factor",
            "function(object) {\n\
             levs <- levels(object)\n\
             if (!is.character(levs)) return(\"factor levels must be \\\"character\\\"\")\n\
             if (d <- anyDuplicated(levs)) return(sprintf(\"duplicated level [%d] in factor\", d))\n\
             TRUE\n\
             }",
        );
        eval_base_binding(
            base_env,
            "factor",
            "function(x = character(), levels, labels = levels,\n\
             exclude = NA, ordered = is.ordered(x), nmax = NA) {\n\
             if (is.null(x)) x <- character()\n\
             nx <- names(x)\n\
             matchAsChar <- is.object(x) ||\n\
                 !(is.character(x) || is.integer(x) || is.logical(x))\n\
             if (matchAsChar) x <- as.character(x)\n\
             if (missing(levels)) {\n\
                 y <- unique(x, nmax = nmax)\n\
                 ind <- order(y)\n\
                 levels <- unique(y[ind])\n\
             }\n\
             force(ordered)\n\
             if (matchAsChar) x <- as.character(x)\n\
             levels <- levels[is.na(match(levels, exclude))]\n\
             f <- match(x, levels)\n\
             if (!is.null(nx)) names(f) <- nx\n\
             if (missing(labels)) {\n\
                 levels(f) <- as.character(levels)\n\
             } else {\n\
                 nlab <- length(labels)\n\
                 if (nlab == length(levels)) {\n\
                     nlevs <- unique(xlevs <- as.character(labels))\n\
                     at <- attributes(f)\n\
                     at$levels <- nlevs\n\
                     f <- match(xlevs, nlevs)[f]\n\
                     attributes(f) <- at\n\
                 } else if (nlab == 1L) {\n\
                     levels(f) <- paste0(labels, seq_along(levels))\n\
                 } else stop(sprintf(\"invalid 'labels'; length %d should be 1 or %d\",\n\
                                     nlab, length(levels)))\n\
             }\n\
             class(f) <- c(if (ordered) \"ordered\", \"factor\")\n\
             f\n\
             }",
        );
        eval_base_binding(
            base_env,
            "extendrange",
            "function(x, r = range(x, na.rm = TRUE), f = 0.05) {\n\
             if(!missing(r) && length(r) != 2)\n\
                 stop(\"'r' must be a \\\"range\\\", hence of length 2\")\n\
             f <- if(length(f) == 1L) c(-f, f) else c(-f[1L], f[2L])\n\
             r + f * diff(r)\n\
             }",
        );

        eval_base_binding(
            base_env,
            "panel.smooth",
            "function(x, y, col = par(\"col\"), bg = NA, pch = par(\"pch\"),\n\
             cex = 1, col.smooth = 2, span = 2/3, iter = 3, ...) {\n\
             points(x, y, pch = pch, col = col, bg = bg, cex = cex)\n\
             ok <- is.finite(x) & is.finite(y)\n\
             if (any(ok))\n\
                 lines(lowess(x[ok], y[ok], f = span, iter = iter), col = col.smooth, ...)\n\
             }",
        );
        eval_base_binding(
            base_env,
            "strheight",
            "function(s, units = \"user\", cex = NULL, ...) {\n\
             if (is.null(cex)) cex <- par(\"cex\")\n\
             h <- par(\"cin\")[2] * cex\n\
             if (units == \"user\") h <- h * diff(par(\"usr\")[3:4]) / par(\"pin\")[2]\n\
             rep(h, length(s))\n\
             }",
        );


        eval_base_binding(
            base_env,
            "as.factor",
            "function(x) {\n\
             if (is.factor(x)) x\n\
             else if (!is.object(x) && is.integer(x)) {\n\
                 levels <- unique.default(x)\n\
                 if (length(levels)) levels <- levels[order(levels)]\n\
                 f <- match(x, levels)\n\
                 levels(f) <- as.character(levels)\n\
                 if (!is.null(nx <- names(x))) names(f) <- nx\n\
                 class(f) <- \"factor\"\n\
                 f\n\
             } else factor(x)\n\
             }",
        );
        eval_base_binding(
            base_env,
            "array",
            "function (data = NA, dim = length(data), dimnames = NULL) {\n\
             if (is.atomic(data) && !is.object(data))\n\
                 return(.Internal(array(data, dim, dimnames)))\n\
             data <- as.vector(data)\n\
             if (is.object(data)) {\n\
                 dim <- as.integer(dim)\n\
                 if (!length(dim)) stop(\"'dim' cannot be of length 0\")\n\
                 vl <- prod(dim)\n\
                 if (length(data) != vl) {\n\
                     if (vl > .Machine$integer.max)\n\
                         stop(\"'dim' specifies too large an array\")\n\
                     data <- rep_len(data, vl)\n\
                 }\n\
                 if (length(dim)) dim(data) <- dim\n\
                 if (is.list(dimnames) && length(dimnames)) dimnames(data) <- dimnames\n\
                 data\n\
             } else .Internal(array(data, dim, dimnames))\n\
             }",
        );
        eval_base_binding(
            base_env,
            "simplify2array",
            include_str!("../mainutils/base_wrappers/simplify2array.R"),
        );



        // GNU sample.R: closures over .Internal(sample)/sample2, not primitives.
        // setMethod("sample", ...) needs a function skeleton (rport-2gpp.2).
        eval_base_binding(
            base_env,
            "sample",
            "function(x, size, replace = FALSE, prob = NULL) {\n\
             if (length(x) == 1L && is.numeric(x) && is.finite(x) && x >= 1) {\n\
                 if (missing(size)) size <- x\n\
                 sample.int(x, size, replace, prob)\n\
             } else {\n\
                 if (missing(size)) size <- length(x)\n\
                 x[sample.int(length(x), size, replace, prob)]\n\
             }\n\
             }",
        );
        eval_base_binding(
            base_env,
            "sample.int",
            "function(n, size = n, replace = FALSE, prob = NULL,\n\
                      useHash = (n > 1e7 && !replace && is.null(prob) && size <= n/2)) {\n\
             stopifnot(length(n) == 1L)\n\
             if (useHash) {\n\
                 stopifnot(is.null(prob), !replace)\n\
                 .Internal(sample2(n, size))\n\
             } else .Internal(sample(n, size, replace, prob))\n\
             }",
        );
        // GNU rev.R: UseMethod so S4 `[` / length methods run (reg-S4.R mapply).
        eval_base_binding(base_env, "rev", "function(x) UseMethod(\"rev\")");
        eval_base_binding(
            base_env,
            "rev.default",
            "function(x) if (length(x)) x[length(x):1L] else x",
        );
        // GNU New-Internal.R: closure over .Internal(is.unsorted), not a primitive.
        // setMethod("is.unsorted", "A", ...) needs that skeleton (reg-S4.R 616).
        eval_base_binding(
            base_env,
            "is.unsorted",
            "function(x, na.rm = FALSE, strictly = FALSE) {\n\
             if (length(x) <= 1L) return(FALSE)\n\
             if (!na.rm && anyNA(x)) return(NA)\n\
             if (na.rm && any(ii <- is.na(x))) x <- x[!ii]\n\
             .Internal(is.unsorted(x, na.rm, strictly))\n\
             }",
        );
        // GNU funprog.R: identity <- function(x) x. setMethod(..., identity)
        // needs a closure, not a primitive (reg-S4.R PR#15691).
        eval_base_binding(base_env, "identity", "function(x) x");
        eval_base_binding(base_env, "is.na<-", "function(x, value) UseMethod(\"is.na<-\")");
        eval_base_binding(
            base_env,
            "is.na<-.default",
            "function(x, value) { x[value] <- NA; x }",
        );
        // GNU table.R: is.table <- function(x) inherits(x, "table")
        eval_base_binding(base_env, "is.table", "function(x) inherits(x, \"table\")");
        eval_base_binding(base_env, "as.table", include_str!("gnu_as_table.R"));
        eval_base_binding(base_env, "table", include_str!("gnu_table.R"));
        eval_base_binding(base_env, "conformToProto", include_str!("gnu_conform_to_proto.R"));
        eval_base_binding(base_env, "strcapture", include_str!("gnu_strcapture.R"));
        eval_base_binding(base_env, "xtabs", include_str!("gnu_xtabs.R"));
        eval_base_binding(base_env, "as.table.default", include_str!("gnu_as_table_default.R"));
        eval_base_binding(base_env, "as.array", include_str!("gnu_as_array.R"));
        eval_base_binding(base_env, "as.array.default", include_str!("gnu_as_array_default.R"));
        eval_base_binding(base_env, "provideDimnames", include_str!("gnu_provide_dimnames.R"));
        eval_base_binding(base_env, "kmeans", include_str!("gnu_kmeans.R"));

        // GNU utils/R/sourceutils.R: getSrcref for rematched S4 methods
        // (reg-S4.R 638). Lives in utils; install in base so source() tests
        // see it without attaching utils.
        eval_base_binding(
            base_env,
            "getSrcref",
            "function(x) {\n\
             if (inherits(x, \"srcref\")) x\n\
             else if (!is.null(srcref <- attr(x, \"srcref\")) ||\n\
                      is.function(x) && !is.null(srcref <- getSrcref(body(x))))\n\
                 srcref\n\
             else if (methods::is(x, \"MethodDefinition\"))\n\
                 getSrcref(unclass(methods::unRematchDefinition(x)))\n\
             }",
        );
        // GNU base/R/srcfile.R: print/as.character of parse srcrefs
        // (reg-S4.R getSrcref of rematched methods).
        eval_base_binding(
            base_env,
            "as.character.srcref",
            "function(x, useSource = TRUE, to = x, ...) {\n\
             srcfile <- attr(x, \"srcfile\")\n\
             lines <- if (!is.null(srcfile)) srcfile$lines\n\
             if (!isTRUE(useSource) || is.null(lines) || !length(lines)) {\n\
               fn <- if (is.null(srcfile)) \"\" else srcfile$filename\n\
               return(paste0(\"<srcref: file \\\"\", fn, \"\\\">\"))\n\
             }\n\
             first <- as.integer(x[1L]); last <- as.integer(x[3L])\n\
             last <- min(last, length(lines))\n\
             if (is.na(first) || is.na(last) || first < 1L || first > last)\n\
               return(character())\n\
             out <- lines[first:last]\n\
             if (length(out)) {\n\
               out[length(out)] <- substring(out[length(out)], 1L, as.integer(x[4L]))\n\
               out[1L] <- substring(out[1L], as.integer(x[2L]))\n\
             }\n\
             out\n\
             }",
        );
        eval_base_binding(
            base_env,
            "print.srcref",
            "function(x, useSource = TRUE, ...) {\n\
             cat(as.character.srcref(x, useSource = useSource), sep = \"\\n\")\n\
             invisible(x)\n\
             }",
        );






        // GNU print.R: print is UseMethod, not a primitive. setMethod("print")
        // uses the closure as the generic skeleton (rport-2gpp.2.3).
        eval_base_binding(base_env, "print", "function(x, ...) UseMethod(\"print\")");
        // GNU stats/R/AIC.R: AIC is UseMethod so AIC.pfit S3 methods run
        // (reg-S4.R 334-343). The former builtin is AIC.default.
        eval_base_binding(
            base_env,
            "AIC",
            "function(object, ..., k = 2) UseMethod(\"AIC\")",
        );
        eval_base_binding(
            base_env,
            "AIC.logLik",
            "function(object, ..., k = 2) -2 * as.numeric(object) + k * attr(object, \"df\")",
        );
        // GNU sort.R: xtfrm is a primitive internal generic; the default
        // method is this closure (unclass numeric, else rank).
        eval_base_binding(
            base_env,
            "xtfrm.default",
            "function(x) {\n\
             y <- if (is.numeric(x)) unclass(x) else as.vector(rank(x, ties.method = \"min\", na.last = \"keep\"))\n\
             if (!is.numeric(y) || ((length(y) != length(x)) && !inherits(x, \"data.frame\")))\n\
                 stop(\"cannot xtfrm 'x'\")\n\
             y\n\
             }",
        );

        // GNU sort.R: order is a closure over .Internal(order), not a
        // primitive. methods::setGeneric needs formals (rport-txofy).
        eval_base_binding(
            base_env,
            "order",
            "function(..., na.last = TRUE, decreasing = FALSE,\n\
             method = c(\"auto\", \"shell\", \"radix\")) {\n\
             z <- list(...)\n\
             if (length(z) == 0L) return(integer())\n\
             method <- match.arg(method)\n\
             if (any(vapply(z, is.object, logical(1L)))) {\n\
                 z <- lapply(z, function(x) if (is.object(x)) as.vector(xtfrm(x)) else x)\n\
                 return(do.call(\"order\", c(z, list(na.last = na.last, decreasing = decreasing, method = method))))\n\
             }\n\
             .Internal(order(na.last, decreasing, ...))\n\
             }",
        );
        // GNU sort.R / mean.R / stats median.R: closures, not primitives.
        eval_base_binding(
            base_env,
            "sort",
            "function(x, decreasing = FALSE, ...) {\n\
             if (!is.logical(decreasing) || length(decreasing) != 1L)\n\
                 stop(\"'decreasing' must be a length-1 logical vector.\\nDid you intend to set 'partial'?\")\n\
             UseMethod(\"sort\")\n\
             }",
        );
        eval_base_binding(
            base_env,
            "sort.default",
            "function(x, decreasing = FALSE, na.last = NA, ...) {\n\
             if (is.object(x))\n\
                 x[order(x, na.last = na.last, decreasing = decreasing)]\n\
             else .Internal(sort(x, decreasing, na.last, ...))\n\
             }",
        );
        eval_base_binding(
            base_env,
            "sort.int",
            "function(x, partial = NULL, na.last = NA, decreasing = FALSE, ...) {\n\
             decreasing <- as.logical(decreasing)\n\
             if (!is.logical(decreasing) || length(decreasing) != 1L || is.na(decreasing))\n\
                 stop(\"'decreasing' must be a length-1 logical vector.\\nDid you intend to set 'partial'?\")\n\
             .Internal(sort(x, decreasing, na.last, ...))\n\
             }",
        );
        eval_base_binding(
            base_env,
            "getOption",
            "function(x, default = NULL) {\n\
             if (missing(default)) .Internal(getOption(x))\n\
             else {\n\
                 ans <- .Internal(getOption(x))\n\
                 if (is.null(ans)) default else ans\n\
             }\n\
             }",
        );
        eval_base_binding(base_env, "diff.ts", include_str!("gnu_diff_ts.R"));
        eval_base_binding(
            base_env,
            "mean",
            "function(x, ...) UseMethod(\"mean\")",
        );
        eval_base_binding(
            base_env,
            "mean.default",
            "function(x, trim = 0, na.rm = FALSE, ...) {\n\
             if (!is.numeric(x) && !is.complex(x) && !is.logical(x)) {\n\
                 warning(\"argument is not numeric or logical: returning NA\")\n\
                 return(NA_real_)\n\
             }\n\
             if (isTRUE(na.rm)) x <- x[!is.na(x)]\n\
             if (!is.numeric(trim) || length(trim) != 1L)\n\
                 stop(\"'trim' must be numeric of length one\")\n\
             n <- length(x)\n\
             if (trim > 0 && n) {\n\
                 if (is.complex(x))\n\
                     stop(\"trimmed means are not defined for complex data\")\n\
                 if (anyNA(x)) return(NA_real_)\n\
                 if (trim >= 0.5) return(median(x, na.rm = FALSE))\n\
                 lo <- floor(n * trim) + 1\n\
                 hi <- n + 1 - lo\n\
                 x <- sort(x)[lo:hi]\n\
             }\n\
             .Internal(mean(x))\n\
             }",
        );
        eval_base_binding(
            base_env,
            "median",
            "function(x, na.rm = FALSE, ...) UseMethod(\"median\")",
        );
        eval_base_binding(
            base_env,
            "median.default",
            "function(x, na.rm = FALSE, ...) {\n\
             if (is.factor(x) || is.data.frame(x)) stop(\"need numeric data\")\n\
             if (length(names(x))) names(x) <- NULL\n\
             if (na.rm) x <- x[!is.na(x)] else if (any(is.na(x))) return(x[NA_integer_])\n\
             n <- length(x)\n\
             if (n == 0L) return(x[NA_integer_])\n\
             half <- (n + 1L) %/% 2L\n\
             if (n %% 2L == 1L) sort(x, partial = half)[half]\n\
             else mean(sort(x, partial = half + 0L:1L)[half + 0L:1L])\n\
             }",
        );



        eval_base_binding(
            base_env,
            "print.default",
            "function(x, digits = NULL, quote = TRUE, na.print = NULL,\n\
             print.gap = NULL, right = FALSE, max = NULL, width = NULL,\n\
             useSource = TRUE, ...) {\n\
             args <- pairlist(digits = digits, quote = quote, na.print = na.print,\n\
                 print.gap = print.gap, right = right, max = max, width = width,\n\
                 useSource = useSource, ...)\n\
             missings <- c(missing(digits), missing(quote), missing(na.print),\n\
                 missing(print.gap), missing(right), missing(max),\n\
                 missing(width), missing(useSource))\n\
             .Internal(print.default(x, args, missings))\n\
             }",
        );
        // GNU New-Internal.R: cbind/rbind are closures around .Internal
        // so the first argument is always deparse.level. cbind2 defaults
        // pass -1L to disable S4 redispatch (bind.c tryS4).
        eval_base_binding(
            base_env,
            "cbind",
            "function(..., deparse.level = 1) .Internal(cbind(deparse.level, ...))",
        );
        eval_base_binding(
            base_env,
            "rbind",
            "function(..., deparse.level = 1) .Internal(rbind(deparse.level, ...))",
        );




        eval_base_binding(
            base_env,
            "formals",
            "function(fun = sys.function(sys.parent()), envir = parent.frame()) {\n\
             if (is.character(fun))\n\
                 fun <- get(fun, mode = \"function\", envir = envir)\n\
             .Internal(formals(fun))\n\
             }",
        );
        eval_base_binding(
            base_env,
            "formalArgs",
            "function(def) names(formals(def))",
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
        eval_base_binding(base_env, "Vectorize", include_str!("gnu_vectorize.R"));
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
            "isSymmetric",
            "function(object, ...) UseMethod(\"isSymmetric\")",
        );
        eval_base_binding(
            base_env,
            "isSymmetric.matrix",
            "function(object, tol = 100 * .Machine$double.eps, tol1 = 8 * tol, trans = \"C\", ...) {\n\
             if (!is.matrix(object)) return(FALSE)\n\
             d <- dim(object)\n\
             if ((n <- d[1L]) != d[2L]) return(FALSE)\n\
             iCplx <- is.complex(object) && trans == \"C\"\n\
             if (n > 1L && length(tol1)) {\n\
                 Cj <- if (iCplx) Conj else identity\n\
                 for (i in unique(c(1L, 2L, n - 1L, n)))\n\
                     if (is.character(all.equal(object[i, ], Cj(object[, i]), tolerance = tol1, ...)))\n\
                         return(FALSE)\n\
             }\n\
             test <- if (iCplx)\n\
                 all.equal.numeric(object, Conj(t(object)), tolerance = tol, ...)\n\
             else\n\
                 all.equal(object, t(object), tolerance = tol, ...)\n\
             isTRUE(test)\n\
             }",
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
        // GNU namespace.R: isNamespaceLoaded(name) .Internal(isRegisteredNamespace(name))
        eval_base_binding(
            base_env,
            "isNamespaceLoaded",
            "function(name) .Internal(isRegisteredNamespace(name))",
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
            "getNamespaceName",
            "function(ns) {\n\
             ns <- asNamespace(ns)\n\
             if (identical(ns, .BaseNamespaceEnv) || identical(ns, baseenv())) \"base\"\n\
             else unname(getNamespaceInfo(ns, \"spec\")[\"name\"])\n\
             }",
        );
        eval_base_binding(
            base_env,
            ".register_print_data_frame",
            "{ registerS3method(\"print\", \"data.frame\", function(x, ...) print.data.frame(x, ...)); TRUE }",
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
        let x = Rf_install_in_current("x");

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
    proto(
        "rank",
        &[
            arg("x"),
            arg_default("na.last", FormalDefault::True),
            arg_default("ties.method", FormalDefault::String("average")),
        ],
        false,
    ),
    proto("rep.int", &[arg("x"), arg("times")], false),
    proto("rep_len", &[arg("x"), arg("length.out")], false),
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
    name == "difftime"
        || LANGUAGE_ELEMENTS.iter().any(|n| *n == name)
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
    fn test_gnu_eval_which_stopifnot_are_base_closures() {
        let mut session = crate::sexp::session::RSession::new();

        let (result, _, _) = session.eval_code_with_output_capture(
            r#"
                exists("eval", baseenv(), inherits = FALSE) &&
                    exists("evalq", baseenv(), inherits = FALSE) &&
                    exists("eval.parent", baseenv(), inherits = FALSE) &&
                    exists("which", baseenv(), inherits = FALSE) &&
                    exists("stopifnot", baseenv(), inherits = FALSE) &&
                    exists("parent.frame", baseenv(), inherits = FALSE) &&
                    identical(typeof(eval), "closure") &&
                    identical(typeof(which), "closure") &&
                    identical(typeof(stopifnot), "closure") &&
                    identical(typeof(parent.frame), "closure") &&
                    identical(eval(1 + 1), 2) &&
                    identical(eval(quote(1 + 1)), 2) &&
                    identical(evalq(a, list(a = 3L)), 3L) &&
                    identical(which(c(TRUE, FALSE, TRUE)), c(1L, 3L)) &&
                    identical(cumsum(c(1+1i, 2+2i)), c(1+1i, 3+3i)) &&
                    is.null(stopifnot(TRUE)) &&
                    inherits(try(stopifnot(FALSE), silent = TRUE), "try-error") &&
                    (function() identical(parent.frame(), .GlobalEnv))() &&
                    identical((function() { x <- 10L; (function() eval.parent(quote(x)))() })(), 10L) &&
                    exists("sys.nframe", baseenv(), inherits = FALSE) &&
                    identical(typeof(sys.nframe), "closure") &&
                    inherits(try(sys.nframe(1), silent = TRUE), "try-error") &&
                    is.integer((function() sys.nframe())())


            "#,
        );
        assert_eq!(
            result
                .expect("GNU eval/which/stopifnot should be base closures")
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
