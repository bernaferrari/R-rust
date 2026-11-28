
## GNU's sessionInfo shape exactly: the version/platform/locale capsule
## answering as a named list with the tzcode_type component the datetime
## regression files branch on — internal exactly as rport's own tzcode.
sessionInfo <- function(package = NULL) {
    z <- list()
    z$R.version <- R.version
    z$platform <- R.version$platform
    z$tzcode_type <- "internal"
    z$running <- "rport"
    z$locale <- Sys.getlocale()
    class(z) <- "sessionInfo"
    z
}
print.sessionInfo <- function(x, locale = TRUE, ...) {
    cat("rport session information\n")
    if (locale) cat("locale:", x$locale, "\n")
    invisible(x)
}
# R-level utils package definitions, evaluated into the utils namespace at
# bootstrap exactly as GNU populates package code. Semantics mirror GNU R's
# utils/R sources; implementations are kept dependency-minimal for the
# primitive surface the pure world carries.

modifyList <- function (x, val, keep.null = FALSE)
{
    stopifnot(is.list(x), is.list(val))
    xnames <- names(x)
    vnames <- names(val)
    vnames <- vnames[nzchar(vnames)]
    if (keep.null) {
        for (v in vnames) x[v] <- list(val[v])
    }
    else {
        for (v in vnames) x[[v]] <- if (v %in% xnames && is.list(x[[v]]) &&
            is.list(val[[v]]))
            modifyList(x[[v]], val[[v]])
        else val[[v]]
    }
    x
}


# GNU's structure printer exactly as utils exports it — the compact
# one-line-per-node description: atomic vectors render their type and
# leading elements, lists recurse through their names with dollar
# indentation, NULL and functions answering their bare labels.
str <- function(object, ...) {
    str.describe <- function(x, prefix) {
        if (is.null(x)) {
            cat(prefix, " NULL\n", sep = "")
        } else if (is.list(x) && !is.data.frame(x) && is.null(dim(x))) {
            if (inherits(x, "data.frame")) {
                cat(prefix, "'data.frame':\n", sep = "")
            } else {
                cat(prefix, "List of ", length(x), "\n", sep = "")
            }
            nms <- names(x)
            for (i in seq_along(x)) {
                label <- if (!is.null(nms) && nzchar(nms[i])) paste0(" $ ", nms[i], ":") else paste0(" [[", i, "]]")
                str.describe(x[[i]], paste0(prefix, label))
            }
        } else if (is.data.frame(x)) {
            cat(prefix, "'data.frame': ", nrow(x), " obs. of ", length(x), " variables\n", sep = "")
            nms <- names(x)
            for (i in seq_along(x)) {
                str.describe(x[[i]], paste0(prefix, " $ ", nms[i], ":"))
            }
        } else if (is.function(x)) {
            cat(prefix, "function\n", sep = "")
        } else if (is.environment(x)) {
            cat(prefix, "<environment>\n", sep = "")
        } else {
            typ <- switch(typeof(x),
                double = "num", integer = "int", logical = "logi",
                character = "chr", complex = "cplx", raw = "raw",
                typeof(x))
            n <- length(x)
            if (n == 0) {
                cat(prefix, " ", typ, "(0) \n", sep = "")
            } else {
                idx <- seq_len(min(n, 5))
                vals <- x[idx]
                body <- if (typeof(x) == "logical") {
                    paste(ifelse(is.na(vals), "NA", ifelse(vals, "TRUE", "FALSE")), collapse = " ")
                } else if (typeof(x) == "character") {
                    paste(ifelse(is.na(vals), "NA", paste0('"', vals, '"')), collapse = " ")
                } else {
                    paste(format(vals), collapse = " ")
                }
                rng <- if (n > 1) paste0("[1:", n, "] ") else ""
                cat(prefix, " ", typ, " ", rng, body, "\n", sep = "")
            }
        }
    }
    str.describe(object, "")
    invisible()
}
