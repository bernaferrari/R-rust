//! init bindings_ops bindings — extracted verbatim from init.rs; binding order preserved.

use super::*;

pub(crate) unsafe fn install(base_env: SEXP) {
    unsafe {
        // GNU datetime.R Ops group methods. The arithmetic primitive is a
        // raw `.default` backend: unit coercion for mixed-unit difftime
        // arithmetic happens here (coerceTimeUnit + NextMethod), exactly
        // like GNU's R-level Ops.difftime. `*.difftime`/`/.difftime` are
        // the specific methods findmethod prefers over the group method.
        eval_base_binding(
            base_env,
            "Ops.difftime",
            include_str!("../gnu_Ops_difftime.R"),
        );
        eval_base_binding(
            base_env,
            "*.difftime",
            include_str!("../gnu_mul_difftime.R"),
        );
        eval_base_binding(
            base_env,
            "/.difftime",
            include_str!("../gnu_div_difftime.R"),
        );
        eval_base_binding(
            base_env,
            "trunc.POSIXt",
            include_str!("../gnu_trunc_POSIXt.R"),
        );
        eval_base_binding(base_env, "seq.POSIXt", include_str!("../gnu_seq_POSIXt.R"));
        eval_base_binding(base_env, "seq.Date", include_str!("../gnu_seq_Date.R"));
        eval_base_binding(
            base_env,
            "pretty.POSIXt",
            include_str!("../gnu_pretty_date.R"),
        );
        eval_base_binding(
            base_env,
            ".amatch_costs",
            r#"function(x = NULL) {
    costs <- c(insertions = 1, deletions = 1, substitutions = 1)
    if (!is.null(x)) {
        x <- as.list(x)
        pos <- pmatch(names(x), names(costs))
        if (anyNA(pos)) {
            warning("unknown cost components ignored")
            x <- x[!is.na(pos)]
        }
        x <- unlist(x)
        if (!all(is.numeric(x)) || any(x < 0))
            stop("cost components must be non-negative")
        costs[pos] <- x
    }
    costs
}"#,
        );
        eval_base_binding(
            base_env,
            ".amatch_bounds",
            include_str!("../gnu_amatch_bounds.R"),
        );
        eval_base_binding(
            base_env,
            "path.package",
            include_str!("../gnu_path_package.R"),
        );
        eval_base_binding(base_env, "Negate", include_str!("../gnu_negate.R"));
        eval_base_binding(
            base_env,
            "summary.default",
            include_str!("../gnu_summary_default.R"),
        );
        eval_base_binding(
            base_env,
            "format.summaryDefault",
            include_str!("../gnu_format_summary.R"),
        );
        eval_base_binding(
            base_env,
            "print.summaryDefault",
            include_str!("../gnu_print_summary.R"),
        );
        eval_base_binding(
            base_env,
            "summary.difftime",
            include_str!("../gnu_summary_difftime.R"),
        );
        eval_base_binding(
            base_env,
            "duplicated.warnings",
            include_str!("../gnu_duplicated_warnings.R"),
        );
        eval_base_binding(
            base_env,
            "unique.warnings",
            include_str!("../gnu_unique_warnings.R"),
        );
        eval_base_binding(
            base_env,
            "summary.warnings",
            include_str!("../gnu_summary_warnings.R"),
        );
        eval_base_binding(
            base_env,
            "[.warnings",
            include_str!("../gnu_subset_warnings.R"),
        );
        eval_base_binding(
            base_env,
            "print.warnings",
            include_str!("../gnu_print_warnings.R"),
        );
        eval_base_binding(
            base_env,
            "print.summary.warnings",
            include_str!("../gnu_print_summary_warnings.R"),
        );
        eval_base_binding(
            base_env,
            "isS3stdGeneric",
            include_str!("../gnu_isS3stdGeneric.R"),
        );
        eval_base_binding(base_env, "isS3method", include_str!("../gnu_isS3method.R"));
        eval_base_binding(base_env, "Reduce", include_str!("../gnu_reduce.R"));
        eval_base_binding(base_env, "axTicks", include_str!("../gnu_axTicks.R"));
        eval_base_binding(base_env, "axisTicks", include_str!("../gnu_axis_ticks.R"));
        eval_base_binding(base_env, "bxp", include_str!("../gnu_bxp.R"));
        eval_base_binding(
            base_env,
            "reformulate",
            include_str!("../gnu_reformulate.R"),
        );
        eval_base_binding(base_env, ".Deprecated", include_str!("../gnu_deprecated.R"));
        eval_base_binding(base_env, ".Defunct", include_str!("../gnu_defunct.R"));
        eval_base_binding(
            base_env,
            "simpleMessage",
            include_str!("../gnu_simple_message.R"),
        );
        eval_base_binding(base_env, "getHook", include_str!("../gnu_userhooks.R"));
        eval_base_binding(base_env, "grepRaw", include_str!("../gnu_grepRaw.R"));
        eval_base_binding(base_env, "symnum", include_str!("../gnu_symnum.R"));
        eval_base_binding(base_env, "write.dcf", include_str!("../gnu_write_dcf.R"));
        eval_base_binding(
            base_env,
            "c.noquote",
            "function(..., recursive = FALSE) structure(NextMethod(\"c\"), class = \"noquote\")",
        );
        eval_base_binding(
            base_env,
            "print.noquote",
            include_str!("../gnu_print_noquote.R"),
        );
        eval_base_binding(base_env, "ls", include_str!("../gnu_ls.R"));
        eval_base_binding(base_env, "objects", include_str!("../gnu_ls.R"));
        eval_base_binding(
            base_env,
            "as.vector.factor",
            include_str!("../gnu_as_vector_factor.R"),
        );
        eval_base_binding(
            base_env,
            "as.list.factor",
            "function(x, ...) { res <- vector(\"list\", length(x)); for (i in seq_along(x)) res[[i]] <- x[[i]]; if (is.null(names(x))) res else `names<-`(res, names(x)) }",
        );
        eval_base_binding(
            base_env,
            "as.Date.default",
            "function(x, ...) { if (inherits(x, \"Date\")) x else if (is.null(x)) structure(numeric(), class = \"Date\") else if (is.logical(x) && all(is.na(x))) structure(as.numeric(x), class = \"Date\") else stop(gettextf(\"do not know how to convert '%s' to class %s\", deparse1(substitute(x)), dQuote(\"Date\")), domain = NA) }",
        );
        eval_base_binding(base_env, "print.Date", include_str!("../gnu_print_Date.R"));
        eval_base_binding(base_env, "prmatrix", include_str!("../gnu_prmatrix.R"));
        eval_base_binding(
            base_env,
            "as.matrix.default",
            include_str!("../gnu_as_matrix_default.R"),
        );
        eval_base_binding(
            base_env,
            "as.matrix.data.frame",
            include_str!("../gnu_as_matrix_data_frame.R"),
        );
        eval_base_binding(base_env, "strwrap", include_str!("../gnu_strwrap.R"));
        eval_base_binding(base_env, "formatDL", include_str!("../gnu_format_dl.R"));
        eval_base_binding(base_env, "file.show", include_str!("../gnu_file_show.R"));
        eval_base_binding(base_env, "try", include_str!("../gnu_try.R"));
        eval_base_binding(base_env, "by", include_str!("../gnu_by.R"));
        eval_base_binding(
            base_env,
            "format.data.frame",
            include_str!("../gnu_format_data_frame.R"),
        );
        eval_base_binding(
            base_env,
            "print.octmode",
            "function(x, ...) { if(length(x)) print(format(x), ...) else cat(\"<0-length octmode>\\n\"); invisible(x) }",
        );
        eval_base_binding(
            base_env,
            "print.hexmode",
            "function(x, ...) { if(length(x)) print(format(x), ...) else cat(\"<0-length hexmode>\\n\"); invisible(x) }",
        );
        eval_base_binding(
            base_env,
            "print.roman",
            "function(x, quote = FALSE, ...) { if(length(x)) print(as.character.roman(x), quote = quote, ...) else cat(\"<0-length roman>\\n\"); invisible(x) }",
        );
        eval_base_binding(
            base_env,
            "as.roman",
            "function(x) { if (is.double(x) || is.logical(x)) x <- as.integer(x); if (!is.integer(x)) stop(\"cannot coerce 'x' to roman\"); if (length(x)) x[x <= 0L | x > 4999L] <- NA; class(x) <- \"roman\"; x }",
        );
        eval_base_binding(
            base_env,
            "person",
            "function(given = NULL, family = NULL, middle = NULL, email = NULL, role = NULL, comment = NULL, first = NULL, last = NULL) structure(list(), class = \"person\")",
        );
        eval_base_binding(
            base_env,
            "print.person",
            "function(x, ...) { if (length(x)) print(format(x, ...)) else cat(\"person()\\n\"); invisible(x) }",
        );
        eval_base_binding(
            base_env,
            "print.bibentry",
            "function(x, ...) { n <- length(x); if (!n) { cl <- class(x)[[1L]]; cat(if (cl == \"bibentry\") \"bibentry()\" else sprintf(\"<0-length %s>\", cl), \"\\n\", sep = \"\") } else { print(format(x, ...)); invisible(x) } }",
        );
        eval_base_binding(
            base_env,
            "citation",
            "function(package = \"base\", lib.loc = NULL, auto = NULL) structure(list(), class = c(\"citation\", \"bibentry\"))",
        );
        eval_base_binding(
            base_env,
            "[.citation",
            "function(x, i, j, drop = TRUE) structure(list(), class = class(x))",
        );
        eval_base_binding(
            base_env,
            "list2DF",
            "function(x = list(), nrow = 0L) { stopifnot(is.list(x), is.null(nrow) || nrow >= 0L); if (n <- length(x)) { if (length(nrow <- unique(lengths(x))) > 1L) stop(\"all variables should have the same length\") } else if (is.null(nrow)) nrow <- 0L; if (is.null(names(x))) names(x) <- character(n); class(x) <- \"data.frame\"; attr(x, \"row.names\") <- .set_row_names(nrow); x }",
        );
        eval_base_binding(
            base_env,
            "download.file",
            "function(url, destfile, method, quiet = FALSE, mode = \"w\", cacheOK = TRUE, extra = getOption(\"download.file.extra\"), headers = NULL, ...) { method <- if (missing(method)) getOption(\"download.file.method\", default = \"auto\") else method; match.arg(method, c(\"auto\", \"internal\", \"libcurl\", \"wget\", \"curl\", \"lynx\")); stop(\"download.file is not implemented\") }",
        );
        eval_base_binding(
            base_env,
            "length<-.POSIXct",
            include_str!("../gnu_lengthgets_POSIXct.R"),
        );
        eval_base_binding(
            base_env,
            "length<-.POSIXlt",
            include_str!("../gnu_lengthgets_POSIXlt.R"),
        );
        eval_base_binding(
            base_env,
            "print.POSIXct",
            include_str!("../gnu_print_POSIXt.R"),
        );
        eval_base_binding(
            base_env,
            "print.POSIXlt",
            include_str!("../gnu_print_POSIXt.R"),
        );
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
             if (missing(levels)) {\n\
                 y <- unique(x, nmax = nmax)\n\
                 ind <- order(y)\n\
                 if (matchAsChar) y <- as.character(y)\n\
                 levels <- unique(y[ind])\n\
             }\n\
             if (d <- anyDuplicated(levels)) stop(sprintf(\"duplicated level [%d] in factor\", d))\n\
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
        eval_base_binding(base_env, "strheight", include_str!("../gnu_strheight.R"));

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
            include_str!("../../mainutils/base_wrappers/simplify2array.R"),
        );
    }
}
