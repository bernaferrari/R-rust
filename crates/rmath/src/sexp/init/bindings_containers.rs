//! init bindings_containers bindings — extracted verbatim from init.rs; binding order preserved.

use super::*;

pub(crate) unsafe fn install(base_env: SEXP) {
    unsafe {
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
            include_str!("../gnu_as_data_frame_vector.R"),
        );
        eval_base_binding(
            base_env,
            "as.data.frame.array",
            include_str!("../gnu_as_data_frame_array.R"),
        );

        for cls in [
            "raw",
            "factor",
            "ordered",
            "integer",
            "logical",
            "numeric",
            "complex",
            "Date",
            "difftime",
            "POSIXct",
            "noquote",
            "numeric_version",
        ] {
            let src = format!(
                "function(x, ...) {{ if (!exists(\".Generic\", inherits = FALSE)) warning(\"Direct call of 'as.data.frame.{cls}()' is deprecated.  Use 'as.data.frame.vector()' or 'as.data.frame()' instead\", call. = TRUE, domain = NA); as.data.frame.vector(x, ...) }}"
            );
            eval_base_binding(base_env, &format!("as.data.frame.{cls}"), &src);
        }
        eval_base_binding(
            base_env,
            "noquote",
            "function(obj) structure(obj, class = \"noquote\")",
        );
        eval_base_binding(
            base_env,
            "numeric_version",
            "function(x, strict = TRUE) { if (inherits(x, \"numeric_version\")) return(x); y <- rep.int(list(integer()), length(x)); ok <- !is.na(x); y[ok] <- lapply(strsplit(as.character(x)[ok], \"[.]\"), as.integer); class(y) <- \"numeric_version\"; y }",
        );
        eval_base_binding(base_env, "as.numeric_version", "numeric_version");
        eval_base_binding(
            base_env,
            "package_version",
            "function(x, strict = TRUE) { y <- numeric_version(x); class(y) <- c(\"package_version\", \"numeric_version\"); y }",
        );

        eval_base_binding(
            base_env,
            "Ops.numeric_version",
            "function(e1, e2) {\n\
             if (nargs() == 1L) stop(gettextf(\"unary '%s' not defined for \\\"numeric_version\\\" objects\", .Generic), domain = NA)\n\
             boolean <- switch(.Generic, \"<\" =, \">\" =, \"==\" =, \"!=\" =, \"<=\" =, \">=\" = TRUE, FALSE)\n\
             if (!boolean) stop(gettextf(\"'%s' not defined for \\\"numeric_version\\\" objects\", .Generic), domain = NA)\n\
             if (!inherits(e1, \"numeric_version\")) e1 <- numeric_version(e1)\n\
             if (!inherits(e2, \"numeric_version\")) e2 <- numeric_version(e2)\n\
             op <- get(.Generic, mode = \"function\")\n\
             op(.Internal(compareNumericVersion(e1, e2)), 0L)\n\
             }",
        );

        eval_base_binding(
            base_env,
            "[.numeric_version",
            "function(x, i, j) { xx <- unclass(x); if (missing(i)) i <- seq_along(xx); y <- if (missing(j)) xx[i] else lapply(xx[i], `[`, j); bad <- vapply(y, function(t) is.null(t) || anyNA(t), NA); if (any(bad)) y[bad] <- rep.int(list(integer()), sum(bad)); class(y) <- class(x); y }",
        );
        eval_base_binding(
            base_env,
            "format.numeric_version",
            "function(x, ...) { x <- unclass(x); y <- rep.int(NA_character_, length(x)); names(y) <- names(x); ind <- lengths(x) > 0L; y[ind] <- unlist(lapply(x[ind], paste, collapse = \".\")); y }",
        );
        eval_base_binding(
            base_env,
            "as.character.numeric_version",
            "function(x, ...) as.character(format(x))",
        );
        eval_base_binding(
            base_env,
            "is.na.numeric_version",
            "function(x) lengths(unclass(x)) == 0L",
        );
        eval_base_binding(
            base_env,
            "[<-.numeric_version",
            "function(x, i, j, value) { y <- unclass(x); if (missing(j)) y[i] <- list(integer()) else y[i] <- Map(`[<-`, y[i], list(j), list(as.integer(value))); class(y) <- class(x); y }",
        );
        eval_base_binding(
            base_env,
            "is.na<-.numeric_version",
            "function(x, value) { x[value] <- list(integer()); x }",
        );
        eval_base_binding(
            base_env,
            "as.data.frame.POSIXlt",
            include_str!("../gnu_as_data_frame_POSIXlt.R"),
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
            include_str!("../gnu_rowNamesDF.R"),
        );
        eval_base_binding(
            base_env,
            "as.data.frame.matrix",
            include_str!("../gnu_as_data_frame_matrix.R"),
        );
        eval_base_binding(
            base_env,
            "as.data.frame.table",
            include_str!("../gnu_as_data_frame_table.R"),
        );
        eval_base_binding(
            base_env,
            "as.data.frame.list",
            include_str!("../gnu_as_data_frame_list.R"),
        );
        eval_base_binding(base_env, "data.frame", include_str!("../gnu_data_frame.R"));
        eval_base_binding(
            base_env,
            "print.data.frame",
            include_str!("../gnu_print_data_frame.R"),
        );
        eval_base_binding(
            base_env,
            "summary.data.frame",
            include_str!("../gnu_summary_data_frame.R"),
        );
        eval_base_binding(
            base_env,
            "summary.matrix",
            include_str!("../gnu_summary_matrix.R"),
        );
        eval_base_binding(base_env, "close", "function(con, ...) UseMethod(\"close\")");
        eval_base_binding(
            base_env,
            "close.connection",
            "function(con, type = \"rw\", ...) .Internal(close(con, type))",
        );
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
             if (!(paste0(\"package:\", package) %in% search()) && !quietly)\n\
                 packageStartupMessage(gettextf(\"Loading required package: %s\", package), domain = NA)\n\
             invisible(.rport_require(package))\n\
             }",
        );
        // GNU duplicated.R / factor.R: closures, not primitives.
        eval_base_binding(base_env, "is.factor", "function(x) inherits(x, \"factor\")");
        eval_base_binding(
            base_env,
            "is.ordered",
            "function(x) inherits(x, \"ordered\")",
        );
        eval_base_binding(
            base_env,
            "unique",
            "function(x, incomparables = FALSE, ...) UseMethod(\"unique\")",
        );
        eval_base_binding(
            base_env,
            "unique.default",
            "function(x, incomparables = FALSE, fromLast = FALSE, nmax = NA, ...) {\n\
             if (!is.object(x)) {\n\
                 if (is.array(x)) x <- as.vector(x)\n\
                 return(.Internal(unique(x, incomparables, fromLast, nmax)))\n\
             }\n\
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
            "unique.POSIXlt",
            "function(x, incomparables = FALSE, ...) x[!duplicated(x, incomparables, ...)]",
        );
        eval_base_binding(
            base_env,
            "duplicated.POSIXlt",
            "function(x, incomparables = FALSE, ...) {\n\
             x <- as.POSIXct(x)\n\
             NextMethod(\"duplicated\", x)\n\
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
             if (length(dots)) invisible(.Internal(data(dots, package, envir)))\n\
             else .Internal(data(dots, package, envir))\n\
             }",
        );
        eval_base_binding(base_env, "levels", "function(x) UseMethod(\"levels\")");
        eval_base_binding(
            base_env,
            "levels.default",
            "function(x) attr(x, \"levels\")",
        );
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
        eval_base_binding(
            base_env,
            "replace",
            "function(x, list, values) { x[list] <- values; x }",
        );
        eval_base_binding(
            base_env,
            "asS4",
            "function(object, flag = TRUE, complete = TRUE) .Internal(setS4Object(object, flag, complete))",
        );
        eval_base_binding(
            base_env,
            "asS3",
            "function(object, flag = TRUE, complete = TRUE) .Internal(setS4Object(object, !as.logical(flag), complete))",
        );
        eval_base_binding(
            base_env,
            "enquote",
            "function(cl) as.call(list(quote(base::quote), cl))",
        );
        eval_base_binding(
            base_env,
            "exists",
            "function (x, where = -1, envir = if (missing(frame)) as.environment(where) else sys.frame(frame), frame, mode = \"any\", inherits = TRUE) .Internal(exists(x, envir, mode, inherits))",
        );
        eval_base_binding(
            base_env,
            ".knownS3Generics",
            "local({ baseGenerics <- c(\"Math\", \"Ops\", \"Summary\", \"Complex\", \"matrixOps\", \"as.character\", \"as.data.frame\", \"as.environment\", \"as.matrix\", \"as.vector\", \"cbind\", \"labels\", \"print\", \"rbind\", \"rep\", \"seq\", \"seq.int\", \"plot\", \"sequence\", \"solve\", \"summary\", \"t\"); utilsGenerics <- c(\"edit\", \"str\"); graphicsGenerics <- c(\"contour\", \"hist\", \"identify\", \"image\", \"lines\", \"pairs\", \"points\", \"text\"); statsGenerics <- c(\"add1\", \"AIC\", \"anova\", \"biplot\", \"coef\", \"confint\", \"deviance\", \"df.residual\", \"drop1\", \"extractAIC\", \"fitted\", \"formula\", \"logLik\", \"model.frame\", \"model.matrix\", \"predict\", \"profile\", \"qqnorm\", \"residuals\", \"se.contrast\", \"terms\", \"update\", \"vcov\"); tmp <- rep.int(c(\"base\", \"utils\", \"graphics\", \"stats\"), c(length(baseGenerics), length(utilsGenerics), length(graphicsGenerics), length(statsGenerics))); names(tmp) <- c(baseGenerics, utilsGenerics, graphicsGenerics, statsGenerics); tmp })",
        );
        eval_base_binding(
            base_env,
            ".internalGenerics",
            "c(\"as.vector\", \"cbind\", \"rbind\", \"unlist\", \"is.unsorted\", \"lengths\", \"nchar\", \"rep.int\", \"rep_len\")",
        );
        eval_base_binding(
            base_env,
            ".S3PrimitiveGenerics",
            "c(\"anyNA\", \"as.character\", \"as.complex\", \"as.double\", \"as.environment\", \"as.integer\", \"as.logical\", \"as.call\", \"as.numeric\", \"as.raw\", \"c\", \"dim\", \"dim<-\", \"dimnames\", \"dimnames<-\", \"is.array\", \"is.finite\", \"is.infinite\", \"is.matrix\", \"is.na\", \"is.nan\", \"is.numeric\", \"length\", \"length<-\", \"levels<-\", \"log2\", \"log10\", \"names\", \"names<-\", \"rep\", \"seq.int\", \"xtfrm\")",
        );
        // GNU eval.R / which.R / stop.R: these are closures over .Internal,
        // not FunTab primitives. Binding them into the base frame makes
        // exists()/as.list(baseenv()) match GNU; .Internal still dispatches
        // through FunTab (eval=11/211). parent.frame must be a wrapper so
        // do_parentframe walks from that extra frame (GNU context.c).
        eval_base_binding(
            base_env,
            "parent.frame",
            include_str!("../gnu_parent_frame.R"),
        );
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
        eval_base_binding(
            base_env,
            "sys.parents",
            "function() .Internal(sys.parents())",
        );
        eval_base_binding(
            base_env,
            "sys.on.exit",
            "function() .Internal(sys.on.exit())",
        );
        eval_base_binding(
            base_env,
            "sys.status",
            "function() list(sys.calls = sys.calls(), sys.parents = sys.parents(), sys.frames = sys.frames())",
        );

        eval_base_binding(base_env, "eval", include_str!("../gnu_eval.R"));
        eval_base_binding(base_env, "evalq", include_str!("../gnu_evalq.R"));
        eval_base_binding(
            base_env,
            "eval.parent",
            include_str!("../gnu_eval_parent.R"),
        );
        eval_base_binding(base_env, "arrayInd", include_str!("../gnu_arrayInd.R"));
        eval_base_binding(base_env, "which", include_str!("../gnu_which.R"));
        eval_base_binding(base_env, "which.min", include_str!("../gnu_which_min.R"));
        eval_base_binding(base_env, "which.max", include_str!("../gnu_which_max.R"));
        eval_base_binding(base_env, "stopifnot", include_str!("../gnu_stopifnot.R"));
        eval_base_binding(base_env, "append", include_str!("../gnu_append.R"));
        eval_base_binding(
            base_env,
            "Sys.setLanguage",
            include_str!("../gnu_set_language.R"),
        );
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
        eval_base_binding(base_env, "strptime", include_str!("../gnu_strptime.R"));

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
        eval_base_binding(base_env, "%||%", "function(x, y) if (is.null(x)) y else x");
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
                     pc <- function(nms) paste0(sQuote(nms), collapse = \", \")\n\
                     .Deprecated(msg = paste0(\"Replacing special names \", pc(attrnames[ok]),\n\
                         \" is deprecated; use \", pc(replace[m[ok]]), \" instead.\"))\n\
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
        eval_base_binding(
            base_env,
            "mostattributes<-",
            include_str!("../gnu_mostattributes.R"),
        );
        eval_base_binding(
            base_env,
            "format.pval",
            include_str!("../gnu_format_pval.R"),
        );
        eval_base_binding(base_env, "format", "function(x, ...) UseMethod(\"format\")");
        eval_base_binding(
            base_env,
            "format.default",
            include_str!("../gnu_format_default.R"),
        );
        eval_base_binding(
            base_env,
            "format.AsIs",
            include_str!("../gnu_format_asis.R"),
        );
        eval_base_binding(base_env, "print.AsIs", include_str!("../gnu_print_asis.R"));
        eval_base_binding(base_env, "prettyNum", include_str!("../gnu_prettyNum.R"));
    }
}
