file.copy <- function(from, to, overwrite = recursive, recursive = FALSE,
                      copy.mode = TRUE, copy.date = FALSE)
{
    if (!(nf <- length(from))) return(logical())
    if (!(nt <- length(to))) stop("no files to copy to")
    if (nt == 1L && dir.exists(to)) {
        if (recursive && to %in% from) stop("attempt to copy a directory to itself")
        return(.Internal(file.copy(from, to, overwrite, recursive, copy.mode, copy.date)))
    }
    if (nf > nt) stop("more 'from' files than 'to' files")
    if (recursive) warning("'recursive' will be ignored as 'to' is not a single existing directory")
    if (nt > nf) from <- rep_len(from, length.out = nt)
    okay <- file.exists(from)
    if (!overwrite) okay[file.exists(to)] <- FALSE
    else {
        dirtofile <- dir.exists(from[okay]) & file.exists(to[okay]) & !dir.exists(to[okay])
        if (any(dirtofile)) {
            warning("cannot overwrite a non-directory with a directory")
            okay[okay] <- !dirtofile
        }
    }
    directories <- dir.exists(from[okay])
    if (any(directories)) {
        warning("directories are omitted unless 'recursive = TRUE'")
        okay[okay] <- !directories
    }
    if (any(from[okay] %in% to[okay])) stop("file can not be copied both 'from' and 'to'")
    if (any(okay)) {
        # The primitive stages a replacement before publishing its destination.
        okay[okay] <- .Internal(file.copy.files(from[okay], to[okay], overwrite, FALSE,
                                         copy.mode, copy.date))
    }
    okay
}
