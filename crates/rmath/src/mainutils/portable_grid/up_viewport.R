function(n=1, recording=TRUE) {
  if (n < 0) stop('must navigate up at least one viewport')
  # as.integer truncates toward 0. A positive count that becomes 0 still
  # moves one viewport; exact 0 means the whole stack and is left unchanged.
  if (n > 0) n <- max(1L, as.integer(n))
  path <- .rport_grid('current.path', NULL)
  parts <- if (is.null(path) || !nzchar(path)) character() else strsplit(path, '::', fixed=TRUE)[[1L]]
  depth <- length(parts)
  if (n == 0) n <- depth
  upPath <- NULL
  if (n > 0) {
    if (n > depth) stop("cannot pop the top-level viewport ('grid' and 'graphics' output mixed?)")
    upPath <- structure(paste(parts[(depth - n + 1L):depth], collapse='::'), class=c('vpPath','path'))
    .rport_grid('up', as.integer(n))
  }
  invisible(upPath)
}
