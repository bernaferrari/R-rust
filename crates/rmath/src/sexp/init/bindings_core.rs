//! init bindings_core bindings — extracted verbatim from init.rs; binding order preserved.

use super::*;

pub(crate) unsafe fn install(base_env: SEXP) {
    unsafe {
        // Placeholder until `retarget_base_closure_envs` publishes the real
        // base namespace (parent `.GlobalEnv`, distinct from this search-path
        // environment). methods extraS4 wrappers (unlist, as.vector, lengths)
        // must live here so implicitGeneric treats them as base functions.
        defineVar(
            Rf_install_in_current(".BaseNamespaceEnv"),
            base_env,
            base_env,
        );

        eval_base_binding(
            base_env,
            ".bincode",
            "function(x, breaks, right = TRUE, include.lowest = FALSE) .Internal(bincode(x, breaks, right, include.lowest))",
        );
        eval_base_binding(
            base_env,
            "rep.int",
            "function(x, times) .Internal(rep.int(x, times))",
        );
        eval_base_binding(
            base_env,
            "rep_len",
            "function(x, length.out) .Internal(rep_len(x, length.out))",
        );

        // GNU: T and F are ordinary symbols bound to TRUE/FALSE, not parser
        // keywords. `quote(F())` and `substitute(F(), list(F=...))` need
        // the symbol, while bare `F` still evaluates to FALSE.
        let t = Rf_ScalarLogical(TRUE);
        let _t = super::super::protect::protect(t);
        defineVar(Rf_install_in_current("T"), t, base_env);
        let f = Rf_ScalarLogical(FALSE);
        let _f = super::super::protect::protect(f);
        defineVar(Rf_install_in_current("F"), f, base_env);
        let device = Rf_mkString(c"null device".as_ptr());
        let _device = super::super::protect::protect(device);
        defineVar(Rf_install_in_current(".Device"), device, base_env);
        let devices = super::super::constructors::Rf_allocVector(SEXPTYPE::VECSXP, 1);
        super::super::accessors::SET_VECTOR_ELT(devices, 0, Rf_mkString(c"null device".as_ptr()));
        defineVar(Rf_install_in_current(".Devices"), devices, base_env);

        // GNU formals.R: alist <- function(...) as.list(sys.call())[-1L]
        // Installed after as.list so parse/eval can see the generic.

        // GNU: as.list <- function(x, ...) UseMethod("as.list")
        // The builtin stays as as.list.default; as.list.function is a
        // separate method so as.list(sum) is list(NULL) and as.list(as.list)
        // keeps the generic formals.
        let as_list_formals = formals_from_specs(&[arg("x"), arg("...")]);
        let _as_list_formals_guard = super::super::protect::protect(as_list_formals);
        let as_list_generic = Rf_mkString(c"as.list".as_ptr());
        let _as_list_generic_guard = super::super::protect::protect(as_list_generic);
        let as_list_body = Rf_lang2(Rf_install_in_current("UseMethod"), as_list_generic);
        let _as_list_body_guard = super::super::protect::protect(as_list_body);
        let as_list_closure =
            crate::mainutils::dstruct::mkCLOSXP(as_list_formals, as_list_body, base_env);
        let _as_list_closure_guard = super::super::protect::protect(as_list_closure);
        defineVar(Rf_install_in_current("as.list"), as_list_closure, base_env);
        eval_base_binding(
            base_env,
            "as.list.default",
            "function (x, ...) if (typeof(x) == \"list\") x else .Internal(as.vector(x, \"list\"))",
        );

        // GNU formals.R. sys.call() forced as as.list's argument names this
        // frame, so a missing tag stays the missing symbol (rport-qc8ct).
        eval_base_binding(base_env, "alist", "function(...) as.list(sys.call())[-1L]");
        eval_base_binding(base_env, "strwidth", include_str!("../gnu_strwidth.R"));
        eval_base_binding(
            base_env,
            "as.graphicsAnnot",
            "function(x) if (is.language(x) || !is.object(x)) x else as.character(x)",
        );
        eval_base_binding(
            base_env,
            "as.personList",
            "function(x) {\n\
                 parts <- strsplit(x, \" \", fixed = TRUE)[[1]]\n\
                 n <- length(parts)\n\
                 list(list(given = parts[-n], family = parts[n]))\n\
             }",
        );
        eval_base_binding(
            base_env,
            "as.single",
            "function(x, ...) UseMethod(\"as.single\")",
        );
        eval_base_binding(
            base_env,
            "as.single.default",
            "function(x, ...) structure(as.double(x), Csingle = TRUE)",
        );
        // GNU format.R: format() is a generic; format.factor relabels via
        // levels and format.default forwards to the natively implemented
        // .Internal(format(...)).
        eval_base_binding(base_env, "format", "function(x, ...) UseMethod(\"format\")");
        eval_base_binding(
            base_env,
            "format.factor",
            "function(x, ...) format(structure(as.character(x), names = names(x), dim = dim(x), dimnames = dimnames(x)), ...)",
        );
        eval_base_binding(
            base_env,
            "format.default",
            "function(x, trim = FALSE, digits = NULL, nsmall = 0L,\n\
                     justify = c(\"left\", \"right\", \"centre\", \"none\"),\n\
                     width = NULL, na.encode = TRUE, scientific = NA,\n\
                     decimal.mark = getOption(\"OutDec\"), ...) {\n\
                 adj <- match(justify, c(\"left\", \"right\", \"centre\", \"none\")) - 1L\n\
                 .Internal(format(x, trim, digits, nsmall, width, adj,\n\
                                  na.encode, scientific, decimal.mark))\n\
             }",
        );
        eval_base_binding(
            base_env,
            "hcl",
            "function(h = 0, c = 35, l = 85, alpha = 1, fixup = TRUE) {\n\
                 n <- max(length(h), length(c), length(l), length(alpha))\n\
                 rep(\"#000000\", n)\n\
             }",
        );
        eval_base_binding(base_env, "mosaicplot", "function(x, ...) NULL");
        eval_base_binding(base_env, "dir", "function(...) list.files(...)");
        eval_base_binding(
            base_env,
            "[.simple.list",
            "function(x, i) structure(unclass(x)[i], class = \"simple.list\")",
        );
        eval_base_binding(
            base_env,
            "mle",
            "function(minuslogl, start, ...) {\n\
                 nm <- names(formals(minuslogl))\n\
                 start <- unlist(start[nm])\n\
                 fn <- function(par) {\n\
                     names(par) <- nm\n\
                     do.call(minuslogl, as.list(par))\n\
                 }\n\
                 fit <- optim(start, fn)\n\
                 par <- fit$par\n\
                 names(par) <- nm\n\
                 list(coefficients = par)\n\
             }",
        );
        eval_base_binding(
            base_env,
            "coef.default",
            "function(object, ...) {\n\
                 if (!is.null(object$coefficients)) object$coefficients else object$coef\n\
             }",
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
        eval_base_binding(base_env, "pairlist", "function(...) as.pairlist(list(...))");
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
             .Internal(vapply(X, FUN, FUN.VALUE, USE.NAMES = USE.NAMES))\n\
             }",
        );
        eval_base_binding(
            base_env,
            "read.dcf",
            "function(file, fields = NULL, all = FALSE, keep.white = NULL) {\n\
             if (is.character(file) && !isTRUE(all)) return(.Internal(readDCF(file, fields, keep.white)))\n\
             stop(\"read.dcf(all = TRUE) is not implemented\")\n\
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
        eval_base_binding(
            base_env,
            "split",
            "function(x, f, drop = FALSE, ...) UseMethod(\"split\")",
        );
        eval_base_binding(
            base_env,
            "split.default",
            include_str!("../gnu_split_default.R"),
        );
        eval_base_binding(
            base_env,
            "interaction",
            include_str!("../gnu_interaction.R"),
        );
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
        eval_base_binding(base_env, "unsplit", include_str!("../gnu_unsplit.R"));
        eval_base_binding(
            base_env,
            "stripchart",
            "function(x, ...) UseMethod(\"stripchart\")",
        );
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
        eval_base_binding(base_env, "tapply", include_str!("../gnu_tapply.R"));
        eval_base_binding(
            base_env,
            "is.data.frame",
            "function(x) inherits(x, \"data.frame\")",
        );
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
    }
}
