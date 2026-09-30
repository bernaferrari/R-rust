function(name, strict=FALSE, recording=TRUE) {
  if (!isTRUE(recording)) stop('unrecorded grid operations are not supported')
  name <- as.character(name)
  parts <- unlist(strsplit(name, '::', fixed=TRUE), use.names=FALSE)
  if (!length(parts) || anyNA(parts) || any(!nzchar(parts)))
    stop('a viewport path must contain at least one viewport name')
  if (!length(strict)) stop("invalid 'strict' value")
  # NA_LOGICAL is nonzero in grid's C search, so only an explicit FALSE is non-strict.
  strict <- as.logical(strict)[1L]
  if (is.na(strict)) strict <- TRUE
  depth <- .rport_grid('down', list(name=paste(parts, collapse='::'), strict=strict))
  invisible(depth)
}
