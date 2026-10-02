{
bibentry <- function(bibtype = NULL, textVersion = NULL, header = NULL,
                     footer = NULL, key = NULL, ..., other = list(),
                     mheader = NULL, mfooter = NULL) {
    dots <- list(...)
    if (is.null(bibtype) && !length(dots) && !length(other))
        return(structure(list(), class = "bibentry"))
    author <- if (length(other) && !is.null(other$author)) other$author else dots$author
    year <- if (length(other) && !is.null(other$year)) other$year else dots$year
    out <- as.character(author)
    attr(out, "year") <- as.character(year)
    class(out) <- "bibentry"
    out
}
format.bibentry <- function(x, macros = NULL, ...) {
    if (!length(x)) return(character())
    author <- as.character(x)[1L]
    if (length(macros)) {
        nm <- names(macros)
        for (i in seq_along(macros)) {
            if (!is.null(nm) && nzchar(nm[i]))
                author <- gsub(paste0("\\", nm[i]), as.character(macros[[i]]), author, fixed = TRUE)
        }
    }
    author <- gsub("\\R", "R", author, fixed = TRUE)
    paste0(author, " (", attr(x, "year"), ").")
}
format.bibentry
}
