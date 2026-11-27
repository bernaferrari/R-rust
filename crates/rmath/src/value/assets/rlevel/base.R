# R-level base definitions, evaluated into the base environment at bootstrap
# exactly as GNU's base package R sources install alongside the primitives.

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
