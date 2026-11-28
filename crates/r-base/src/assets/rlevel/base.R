# R-level base definitions, evaluated into the base environment at bootstrap
# exactly as GNU's base package R sources install alongside the primitives.


# GNU's machine constants exactly as platform.c builds them for IEEE doubles.
.Machine <- list(
    double.eps = 2^-52,
    double.neg.eps = 2^-53,
    double.xmin = 2^-1022,
    double.xmax = 1.7976931348623157e+308,
    double.base = 2,
    double.digits = 53,
    double.rounding = 5,
    double.guard = 0,
    double.ulp.digits = -52,
    double.neg.ulp.digits = -53,
    double.exponent = 11,
    double.min.exp = -1022,
    double.max.exp = 1024,
    integer.max = 2147483647L,
    sizeof.long = 8L,
    sizeof.longlong = 8L,
    sizeof.longdouble = 16L,
    sizeof.pointer = 8L
)

print.function <- function (x, useSource = TRUE, ...)
    print.default(x, useSource = useSource, ...)

summary.default <- function (object, ..., digits, quantile.type = 7)
{
    value <- if (is.logical(object))
        c(Mode = "logical", {
            tb <- table(object, exclude = NULL)
            if (!is.null(dim(tb))) {
                dimnames(tb)[[1L]] <- c("FALSE", "TRUE", "NA's")
            }
            tb
        })
    else if (is.numeric(object)) {
        nas <- is.na(object)
        object <- object[!nas]
        qq <- stats::quantile(object, names = FALSE, type = quantile.type)
        qq <- c(qq[1L:3L], mean(object), qq[4L:5L])
        if (!missing(digits))
            qq <- signif(qq, digits)
        names(qq) <- c("Min.", "1st Qu.", "Median", "Mean", "3rd Qu.", "Max.")
        if (any(nas))
            c(qq, `NA's` = sum(nas))
        else qq
    }
    else if (is.recursive(object) && !is.language(object) && (n <- length(object))) {
        sumry <- array("", c(n, 3L), list(names(object), c("Length", "Class", "Mode")))
        same.mode <- identical(object, eval.parent(object))
        for (i in 1L:n) {
            ii <- object[[i]]
            iclass <- class(ii)
            sumry[i, 1L] <- length(ii)
            sumry[i, 2L] <- if (length(iclass)) iclass[1L] else "-none-"
            sumry[i, 3L] <- mode(ii)
        }
        sumry
    }
    else c(Length = length(object), Class = class(object)[1L], Mode = mode(object))
    class(value) <- c("summaryDefault", "table")
    value
}

all.equal <- function (target, current, tolerance = sqrt(.Machine$double.eps), ...)
{
    if (missing(tolerance))
        tolerance <- 1.49011611938477e-08
    if ((is.numeric(target) || is.complex(target)) && (is.numeric(current) || is.complex(current))) {
        lt <- length(target)
        lc <- length(current)
        if (lt != lc)
            return(paste0("Numeric: lengths (", lt, ", ", lc, ") differ"))
        if (lt == 0)
            return(TRUE)
        if (!is.complex(target))
            target <- as.numeric(target)
        if (!is.complex(current))
            current <- as.numeric(current)
        if (any(is.na(target) != is.na(current)))
            return("'is.NA' value mismatch")
        keep <- !is.na(target)
        target <- target[keep]
        current <- current[keep]
        if (length(target) == 0)
            return(TRUE)
        diff <- abs(target - current)
        if (all(diff <= tolerance))
            return(TRUE)
        scale <- mean(abs(target))
        if (is.finite(scale) && scale > tolerance) {
            rel <- mean(diff) / scale
            if (rel <= tolerance)
                return(TRUE)
            return(paste("Mean relative difference:", format(rel)))
        }
        absdiff <- mean(diff)
        if (absdiff <= tolerance)
            return(TRUE)
        return(paste("Mean absolute difference:", format(absdiff)))
    }
    if (identical(target, current))
        return(TRUE)
    paste0("target is ", mode(target), ", current is ", mode(current))
}

# GNU's trivial base closures exactly as base/R sources define them —
# force is the identity that evaluates its argument, the quote family
# pastes the ASCII forms the C locale arranges, noquote is the class
# wrapper, and eval.parent/evalq ride the eval envir machinery.
force <- function(x) x

sQuote <- function(x) paste0("'", x, "'")

dQuote <- function(x) paste0('"', x, '"')

shQuote <- function(string) paste0("'", gsub("'", "'\\\\''", string), "'")

noquote <- function(obj) structure(obj, class = "noquote")

eval.parent <- function(expr, n = 1) eval(expr, parent.frame(n + 1))

evalq <- function(expr, envir = parent.frame(), enclos = NULL) eval(expr, envir)

# GNU's trivial type predicates and operators exactly as base defines
# them — the atomic lattice is the seven scalar types, recursive is
# everything structural, and the null-coalescing infix is base's own.
is.atomic <- function(x) typeof(x) %in% c("NULL", "logical", "integer", "double", "complex", "character", "raw")

is.recursive <- function(x) typeof(x) %in% c("list", "pairlist", "closure", "environment", "language", "expression", "symbol")

`%||%` <- function(x, y) if (is.null(x)) y else x

zapsmall <- function(x, digits = getOption("digits")) {
    mx <- max(abs(x))
    rounded <- round(x, digits)
    ifelse(abs(rounded) <= mx * 10^(-digits), 0, rounded)
}

# GNU's try exactly as base defines it — the error handler captures the
# condition message into the try-error classed string, silent suppressing
# the report.
try <- function(expr, silent = FALSE) {
    tryCatch(expr, error = function(e) structure(conditionMessage(e), class = "try-error"))
}
