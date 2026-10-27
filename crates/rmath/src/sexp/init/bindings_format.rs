//! init bindings_format bindings — extracted verbatim from init.rs; binding order preserved.

use super::*;

pub(crate) unsafe fn install(base_env: SEXP) {
    unsafe {
        // GNU base/format.R formatC: argument checking, character padding
        // and prettyNum post-processing live at the R level; the builtin
        // registered under the same name serves only the 7-argument
        // .Internal(formatC(...)) convention from util.c.
        eval_base_binding(base_env, "formatC", include_str!("../gnu_formatC.R"));
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
        eval_base_binding(base_env, "diff", "function(x, ...) UseMethod(\"diff\")");
        eval_base_binding(
            base_env,
            "diff.default",
            include_str!("../gnu_diff_default.R"),
        );
        eval_base_binding(base_env, "curve", include_str!("../gnu_curve.R"));
        eval_base_binding(
            base_env,
            "proportions",
            include_str!("../gnu_proportions.R"),
        );
        eval_base_binding(base_env, "prop.table", "proportions");
        eval_base_binding(base_env, "marginSums", include_str!("../gnu_margin_sums.R"));
        eval_base_binding(base_env, "C_PDF", "\"C_PDF\"");
        eval_base_binding(
            base_env,
            "pdf",
            "function(file = \"Rplots.pdf\", ...) .External(C_PDF, file)",
        );
        eval_base_binding(base_env, "dev.off", "function(...) 1L");
        eval_base_binding(base_env, "postscript", "function(...) invisible(NULL)");
        eval_base_binding(base_env, "legend", "function(...) invisible(NULL)");
        eval_base_binding(base_env, "dev.hold", "function(...) invisible(NULL)");
        eval_base_binding(base_env, "dev.flush", "function(...) invisible(NULL)");
        eval_base_binding(base_env, "frame", "function(...) invisible(NULL)");
        eval_base_binding(base_env, "as.null", "function(x) NULL");
        eval_base_binding(base_env, "mtext", "function(...) graphics::mtext(...)");
        eval_base_binding(base_env, "duplicated", include_str!("../gnu_duplicated.R"));
        eval_base_binding(
            base_env,
            "duplicated.default",
            include_str!("../gnu_duplicated_default.R"),
        );
        eval_base_binding(
            base_env,
            "duplicated.data.frame",
            include_str!("../gnu_duplicated_data_frame.R"),
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
        eval_base_binding(base_env, "xy.coords", include_str!("../gnu_xy_coords.R"));
        eval_base_binding(base_env, "fix", include_str!("../gnu_fix.R"));
        eval_base_binding(base_env, "edit", "function(name, ...) name");
        eval_base_binding(base_env, "image", "function(...) invisible(NULL)");
        eval_base_binding(
            base_env,
            "contour",
            "function(x, ...) { lv <- list(...)$levels; if (!is.null(lv)) { bad <- which(!is.finite(lv)); if (length(bad)) stop(sprintf('non-finite level values: levels[%d] = %g', bad[1L], lv[bad[1L]])) }; invisible(NULL) }",
        );
        eval_base_binding(base_env, "apropos", include_str!("../gnu_apropos.R"));
        eval_base_binding(base_env, "persp", "function(...) invisible(NULL)");
        eval_base_binding(
            base_env,
            "heat.colors",
            "function(n, ...) rep(\"#FF0000\", n)",
        );
        eval_base_binding(base_env, "rainbow", "function(n, ...) rep(\"#FF0000\", n)");
        eval_base_binding(base_env, "plot", "function(x, y, ...) UseMethod(\"plot\")");
        eval_base_binding(
            base_env,
            "plot.formula",
            include_str!("../gnu_plot_formula.R"),
        );
        eval_base_binding(base_env, "sunflowerplot", "function(...) invisible(NULL)");
        eval_base_binding(
            base_env,
            "plot.data.frame",
            include_str!("../gnu_plot_data_frame.R"),
        );
        eval_base_binding(base_env, "colorRamp", include_str!("../gnu_color_ramp.R"));
        eval_base_binding(
            base_env,
            "colorRampPalette",
            include_str!("../gnu_color_ramp_palette.R"),
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
            include_str!("../gnu_subset_data_frame.R"),
        );
        eval_base_binding(base_env, "relist", include_str!("../gnu_relist.R"));
        eval_base_binding(
            base_env,
            "relist.default",
            include_str!("../gnu_relist_default.R"),
        );
        eval_base_binding(
            base_env,
            "relist.list",
            include_str!("../gnu_relist_list.R"),
        );
        eval_base_binding(base_env, "outer", include_str!("../gnu_outer.R"));
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
        eval_base_binding(
            base_env,
            "getAnywhere",
            include_str!("../gnu_getAnywhere.R"),
        );
        eval_base_binding(base_env, "prompt", include_str!("../gnu_prompt.R"));
        eval_base_binding(base_env, "shQuote", include_str!("../gnu_shquote.R"));
        eval_base_binding(base_env, "?", "{ `?` <- function(e1, e2) NULL; `?` }");
        eval_base_binding(base_env, "help", "{ help <- function(...) NULL; help }");
        eval_base_binding(
            base_env,
            "prompt.default",
            include_str!("../gnu_prompt_default.R"),
        );
        eval_base_binding(
            base_env,
            "deparse1",
            "function(expr, collapse = \" \", width.cutoff = 500L, ...) paste(deparse(expr, width.cutoff, ...), collapse = collapse)",
        );
        eval_base_binding(base_env, "srcfile", include_str!("../gnu_srcfile.R"));
        eval_base_binding(base_env, "R.home", include_str!("../gnu_r_home.R"));
        eval_base_binding(
            base_env,
            "as.expression",
            "function(x, ...) UseMethod(\"as.expression\")",
        );
        eval_base_binding(
            base_env,
            "as.expression.default",
            "function(x, ...) .Internal(as.vector(x, \"expression\"))",
        );
        eval_base_binding(
            base_env,
            "labels",
            "function(object, ...) UseMethod(\"labels\")",
        );
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
        eval_base_binding(
            base_env,
            "within",
            "function(data, expr, ...) UseMethod(\"within\")",
        );
        eval_base_binding(
            base_env,
            "within.data.frame",
            include_str!("../gnu_within_data_frame.R"),
        );
        eval_base_binding(
            base_env,
            "within.list",
            include_str!("../gnu_within_list.R"),
        );
        eval_base_binding(
            base_env,
            "rbind.data.frame",
            include_str!("../gnu_rbind_data_frame.R"),
        );
        eval_base_binding(base_env, ".checkHT", include_str!("../gnu_checkHT.R"));
        eval_base_binding(base_env, "head", "function(x, ...) UseMethod(\"head\")");
        eval_base_binding(
            base_env,
            "head.default",
            include_str!("../gnu_head_default.R"),
        );
        eval_base_binding(base_env, "head.array", include_str!("../gnu_head_array.R"));
        eval_base_binding(base_env, "head.matrix", include_str!("../gnu_head_array.R"));
        eval_base_binding(base_env, "tail", "function(x, ...) UseMethod(\"tail\")");
        eval_base_binding(
            base_env,
            "tail.default",
            include_str!("../gnu_tail_default.R"),
        );
        eval_base_binding(base_env, "tail.array", include_str!("../gnu_tail_array.R"));
        eval_base_binding(base_env, "tail.matrix", include_str!("../gnu_tail_array.R"));
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
        eval_base_binding(
            base_env,
            "reorder",
            "function(x, ...) if (inherits(x, \"dendrogram\")) .rport_reorder_dendrogram(x, ..1) else .rport_reorder_default(x, ..1)",
        );
        eval_base_binding(
            base_env,
            "as.hclust",
            "function(x, ...) UseMethod(\"as.hclust\")",
        );
        eval_base_binding(
            base_env,
            "as.hclust.default",
            "function(x, ...) {\n\
             if (inherits(x, \"hclust\")) x else\n\
             stop(gettextf(\"argument 'x' cannot be coerced to class %s\", dQuote(\"hclust\")), domain = NA)\n\
             }",
        );
        eval_base_binding(base_env, "tar", include_str!("../gnu_tar.R"));
        eval_base_binding(base_env, "kronecker", include_str!("../gnu_kronecker.R"));
        eval_base_binding(base_env, "ps.options", "function(...) list(onefile = TRUE)");
        eval_base_binding(base_env, "match.fun", include_str!("../gnu_match_fun.R"));
        eval_base_binding(
            base_env,
            "summaryRprof",
            include_str!("../gnu_summary_rprof.R"),
        );
        eval_base_binding(
            base_env,
            "subset.matrix",
            include_str!("../gnu_subset_matrix.R"),
        );
        eval_base_binding(base_env, "kappa", include_str!("../gnu_kappa.R"));
        eval_base_binding(
            base_env,
            "kappa.default",
            include_str!("../gnu_kappa_default.R"),
        );
        eval_base_binding(base_env, "kappa.lm", include_str!("../gnu_kappa_lm.R"));
        eval_base_binding(base_env, "kappa.qr", include_str!("../gnu_kappa_qr.R"));
        eval_base_binding(base_env, ".kappa_tri", include_str!("../gnu_kappa_tri.R"));
        eval_base_binding(base_env, "merge", include_str!("../gnu_merge.R"));
        eval_base_binding(
            base_env,
            "merge.default",
            include_str!("../gnu_merge_default.R"),
        );
        eval_base_binding(
            base_env,
            "merge.data.frame",
            include_str!("../gnu_merge_data_frame.R"),
        );
        eval_base_binding(
            base_env,
            "maintainer",
            "function(pkg) { force(pkg); desc <- try(packageDescription(pkg), silent = TRUE); if (is.list(desc)) gsub(\"\\n\", \" \", desc$Maintainer, fixed = TRUE) else NA_character_ }",
        );
        eval_base_binding(base_env, "graphics.off", "function() invisible()");
        eval_base_binding(
            base_env,
            "dev.interactive",
            "function(orNone = FALSE) { if (!interactive()) FALSE else if (isTRUE(orNone) && identical(.Device, \"null device\")) TRUE else isTRUE(interactive()) }",
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
        eval_base_binding(
            base_env,
            "attr.all.equal",
            include_str!("../gnu_attr_all_equal.R"),
        );
        eval_base_binding(
            base_env,
            "count.fields",
            "function(file, sep = \"\", quote = \"\\\"'\", skip = 0,\n         blank.lines.skip = TRUE, comment.char = \"#\")\n{\n    if(is.character(file)) {\n        file <- file(file)\n        on.exit(close(file))\n    }\n    if(!inherits(file, \"connection\"))\n        stop(\"'file' must be a character string or connection\")\n    if (isOpen(file) == FALSE) open(file, \"rt\")\n    .External(C_countfields, file, sep, quote, skip, blank.lines.skip,\n              comment.char)\n}\n",
        );
        eval_base_binding(base_env, "C_countfields", "\"C_countfields\"");
        eval_base_binding(base_env, "C_edit", "\"C_edit\"");
        eval_base_binding(base_env, "C_tzcode_type", "\"C_tzcode_type\"");
        eval_base_binding(base_env, "osVersion", "\"macOS\"");
        eval_base_binding(base_env, "C_readtablehead", "\"C_readtablehead\"");
        eval_base_binding(base_env, "C_octsize", "\"C_octsize\"");
        eval_base_binding(base_env, "C_crc64", "\"C_crc64\"");
        eval_base_binding(base_env, "C_runmed", "\"C_runmed\"");
        eval_base_binding(
            base_env,
            "suppressWarnings",
            "function (expr, classes = \"warning\") { withCallingHandlers(expr, warning = function(w) if (inherits(w, classes)) tryInvokeRestart(\"muffleWarning\")) }",
        );
        eval_base_binding(
            base_env,
            "suppressMessages",
            "function (expr, classes = \"message\") withCallingHandlers(expr, message = function(c) if (inherits(c, classes)) tryInvokeRestart(\"muffleMessage\"))",
        );
        eval_base_binding(
            base_env,
            "suppressPackageStartupMessages",
            "function (expr) withCallingHandlers(expr, packageStartupMessage = function(c) tryInvokeRestart(\"muffleMessage\"))",
        );
        eval_base_binding(base_env, "sub", include_str!("../gnu_sub.R"));
        eval_base_binding(base_env, "gsub", include_str!("../gnu_gsub.R"));
        eval_base_binding(base_env, "grep", include_str!("../gnu_grep.R"));
        eval_base_binding(base_env, "grepl", include_str!("../gnu_grepl.R"));
        eval_base_binding(base_env, "regexpr", include_str!("../gnu_regexpr.R"));
        eval_base_binding(base_env, "gregexpr", include_str!("../gnu_gregexpr.R"));
        eval_base_binding(base_env, "regmatches", include_str!("../gnu_regmatches.R"));
        eval_base_binding(base_env, "read.fwf", include_str!("../gnu_read_fwf.R"));
        eval_base_binding(
            base_env,
            "regmatches<-",
            include_str!("../gnu_regmatches_set.R"),
        );
        eval_base_binding(
            base_env,
            "summary.connection",
            "function(object, ...) .Internal(summary.connection(object))",
        );
        eval_base_binding(
            base_env,
            "getAllConnections",
            "function() .Internal(getAllConnections())",
        );
        eval_base_binding(
            base_env,
            "getConnection",
            "function(what) .Internal(getConnection(what))",
        );
        eval_base_binding(
            base_env,
            "showConnections",
            "function(all = FALSE) {\n\
             set <- getAllConnections()\n\
             if (!all) set <- set[set > 2L]\n\
             ans <- matrix(\"\", length(set), 7L)\n\
             for (i in seq_along(set)) ans[i, ] <- unlist(summary.connection(set[i]))\n\
             rownames(ans) <- set\n\
             colnames(ans) <- c(\"description\", \"class\", \"mode\", \"text\", \"isopen\",\n\
                                \"can read\", \"can write\")\n\
             if (!all) ans[ans[, 5L] == \"opened\", , drop = FALSE] else ans[, , drop = FALSE]\n\
             }",
        );
        eval_base_binding(
            base_env,
            "srcfilecopy",
            include_str!("../gnu_srcfilecopy.R"),
        );

        eval_base_binding(
            base_env,
            ".traceback",
            include_str!("../gnu_dot_traceback.R"),
        );
        eval_base_binding(base_env, "traceback", include_str!("../gnu_traceback.R"));
        eval_base_binding(
            base_env,
            "get_all_vars",
            include_str!("../gnu_get_all_vars.R"),
        );
        eval_base_binding(base_env, "poly", include_str!("../gnu_poly.R"));
        eval_base_binding(base_env, "polym", include_str!("../gnu_polym.R"));
        eval_base_binding(
            base_env,
            "predict.poly",
            include_str!("../gnu_predict_poly.R"),
        );
        eval_base_binding(
            base_env,
            "makepredictcall.poly",
            include_str!("../gnu_makepredictcall_poly.R"),
        );
        eval_base_binding(
            base_env,
            "all.equal.character",
            include_str!("../gnu_all_equal_character.R"),
        );
        eval_base_binding(
            base_env,
            "all.equal.numeric",
            include_str!("../gnu_all_equal_numeric.R"),
        );
        eval_base_binding(
            base_env,
            "all.equal.list",
            include_str!("../gnu_all_equal_list.R"),
        );
        eval_base_binding(
            base_env,
            "all.equal.condition",
            "function(target, current, ...) {\n  if (!inherits(target, \"condition\") || !inherits(current, \"condition\"))\n    return(\"target and current are not both conditions\")\n  msg <- all.equal(class(target), class(current))\n  if (!isTRUE(msg)) return(c(\"Classes differ:\", msg))\n  msg <- all.equal(conditionMessage(target), conditionMessage(current))\n  if (!isTRUE(msg)) return(c(\"Messages differ:\", msg))\n  TRUE\n}",
        );
        eval_base_binding(
            base_env,
            ".handleSimpleError",
            include_str!("../gnu_handle_simple_error.R"),
        );
        eval_base_binding(
            base_env,
            "all.equal.default",
            include_str!("../gnu_all_equal_default.R"),
        );
        eval_base_binding(
            base_env,
            "all.equal.language",
            include_str!("../gnu_all_equal_language.R"),
        );
        eval_base_binding(
            base_env,
            "all.equal.function",
            include_str!("../gnu_all_equal_function.R"),
        );
        eval_base_binding(
            base_env,
            "all.equal.environment",
            include_str!("../gnu_all_equal_environment.R"),
        );
        eval_base_binding(
            base_env,
            "all.equal.formula",
            include_str!("../gnu_all_equal_formula.R"),
        );
        eval_base_binding(
            base_env,
            "all.equal.raw",
            include_str!("../gnu_all_equal_raw.R"),
        );
        eval_base_binding(base_env, "all.equal.logical", "all.equal.raw");
        eval_base_binding(
            base_env,
            "all.equal.factor",
            include_str!("../gnu_all_equal_factor.R"),
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
        eval_base_binding(
            base_env,
            "row.names",
            "function(x) UseMethod(\"row.names\")",
        );
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
        eval_base_binding(base_env, "chkDots", include_str!("../gnu_chkDots.R"));
        eval_base_binding(
            base_env,
            "seq.default",
            include_str!("../gnu_seq_default.R"),
        );
        eval_base_binding(base_env, "units", "function(x) UseMethod(\"units\")");
        eval_base_binding(
            base_env,
            "units<-",
            "function(x, value) UseMethod(\"units<-\")",
        );
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
            include_str!("../gnu_units_difftime.R"),
        );
        eval_base_binding(
            base_env,
            "[<-.difftime",
            include_str!("../gnu_subset_assign_difftime.R"),
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
    }
}
