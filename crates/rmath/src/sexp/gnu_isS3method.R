{
isS3method <- function(method, f, class, envir = parent.frame()) {
    if (missing(method)) {
        method <- paste(f, class, sep = ".")
    } else {
        f.c <- strsplit(method, ".", fixed = TRUE)[[1L]]
        nfc <- length(f.c)
        if (nfc < 2L || !is.character(f.c) || !nzchar(f.c[[1L]]))
            return(FALSE)
    }
    FALSE
}
isS3method
}
