{
write.dcf <- function(x, file = "", append = FALSE, useBytes = FALSE,
                      indent = 0.1 * getOption("width"),
                      width = 0.9 * getOption("width"),
                      keep.white = NULL) {
    if (is.character(file)) {
        if (!nzchar(file)) file <- stdout()
        else {
            file <- file(file, if (append) "a" else "w")
            on.exit(close(file))
        }
    }
    if (!inherits(file, "connection"))
        stop("'file' must be a character string or connection")
    if (!is.data.frame(x))
        x <- as.data.frame(x, optional = TRUE, stringsAsFactors = FALSE)
    indent <- as.integer(indent)
    width <- as.integer(width)
    lines <- character()
    for (j in seq_along(x)) {
        val <- as.character(x[[j]])
        if (length(val) != 1L || is.na(val)) next
        tag <- names(x)[j]
        words <- strsplit(val, "[ \t]+", perl = TRUE)[[1]]
        words <- words[nzchar(words)]
        prefix <- paste(rep(" ", indent), collapse = "")
        if (nchar(tag) + 2L > width) {
            lines <- c(lines, paste0(tag, ":"))
            cur <- prefix
        } else {
            cur <- paste0(tag, ": ")
        }
        for (w in words) {
            trial <- if (endsWith(cur, " ") || endsWith(cur, ":")) paste0(cur, w) else paste0(cur, " ", w)
            if (nchar(trial) > width && !endsWith(cur, ":") && cur != prefix) {
                lines <- c(lines, cur)
                cur <- paste0(prefix, w)
            } else {
                cur <- trial
            }
        }
        if (nzchar(cur)) lines <- c(lines, cur)
    }
    lines <- gsub("\n \\.([^\n])", "\n  .\\1", paste(lines, collapse = "\n"), perl = TRUE)
    # A continuation that is a single token starting with '.' gained only the indent.
    lines <- strsplit(lines, "\n", fixed = TRUE)[[1]]
    lines <- sub("^ \\.", "  .", lines)
    writeLines(lines, file, useBytes = useBytes)
    invisible(NULL)
}
write.dcf
}
