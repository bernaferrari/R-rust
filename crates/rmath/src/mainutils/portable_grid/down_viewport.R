function(name, strict=FALSE, recording=TRUE) {
  name <- as.character(name)
  parts <- unlist(strsplit(name, '::', fixed=TRUE), use.names=FALSE)
  if (!length(parts) || anyNA(parts) || any(!nzchar(parts)))
    stop('a viewport path must contain at least one viewport name')
  if (!length(strict)) stop("invalid 'strict' value")
  # NA_LOGICAL is nonzero in grid's C search, so only an explicit FALSE is non-strict.
  strict <- as.logical(strict)[1L]
  if (is.na(strict)) strict <- TRUE
  # GNU records the viewports actually entered, not the name that was asked
  # for. A non-strict downViewport("c") through a::b::c records a, b, and c.
  before <- .rport_grid('current.path', NULL)
  depth <- .rport_grid('down', list(name=paste(parts, collapse='::'), strict=strict))
  if (isTRUE(recording)) {
    after <- .rport_grid('current.path', NULL)
    prefix <- if (is.null(before) || !nzchar(before)) '' else paste0(before, '::')
    rel <- if (!nzchar(prefix)) after else if (!is.null(after) && substring(after, 1L, nchar(prefix)) == prefix) substring(after, nchar(prefix) + 1L) else after
    if (!is.null(rel) && nzchar(rel)) .rport_grid('record.down', rel)
  }
  invisible(depth)
}
