function(x, row.names = NULL, optional = FALSE, validRN = TRUE, ...,
         nm = deparse1(substitute(x)))
{
    force(nm)
    nrows <- length(x)
    if (is.null(row.names)) {
        if (nrows == 0L)
            row.names <- character()
        else if (length(row.names <- names(x)) != nrows || anyDuplicated(row.names))
            row.names <- .set_row_names(nrows)
        else if (validRN && anyNA(row.names)) stop("row names contain missing values")
    } else if (validRN && anyNA(row.names)) {
        stop("row names contain missing values")
    } else if (!(is.character(row.names) || is.integer(row.names)) || length(row.names) != nrows) {
        stop(gettextf("'row.names' is not a character or integer vector of length %d", nrows),
             domain = NA)
    }
    if (!is.null(names(x))) names(x) <- NULL
    value <- list(x)
    if (!optional) names(value) <- nm
    structure(value, row.names = row.names, class = "data.frame")
}
