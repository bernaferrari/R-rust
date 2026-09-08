function(...) {
    names <- list(...)
    if (!length(names) || any(!vapply(names, is.character, logical(1))))
        stop("invalid grob names")
    names <- unlist(strsplit(unlist(names), "::", fixed = TRUE), use.names = FALSE)
    if (!length(names) || any(!nzchar(names)))
        stop("a 'grob' path must contain at least one 'grob' name")
    structure(list(path = if (length(names) > 1L)
                       paste(names[-length(names)], collapse = "::") else NULL,
                   name = names[length(names)], n = length(names)),
              class = c("gPath", "path"))
}
