function(n=1, recording=TRUE) {
  if (n < 0) stop('must navigate up at least one viewport')
  # The recorded count is the argument, except exact 0 which records the
  # depth. The move uses as.integer, and a positive truncation to 0 still
  # moves one viewport. The returned path is the viewports that were left.
  path <- .rport_grid('current.path', NULL)
  parts <- if (is.null(path) || !nzchar(path)) character() else strsplit(path, '::', fixed=TRUE)[[1L]]
  depth <- length(parts)
  recorded <- if (n == 0) depth else n
  nav <- if (n == 0) depth else if (n > 0) max(1L, as.integer(n)) else n
  upPath <- NULL
  if (nav > 0) {
    if (nav > depth) stop("cannot pop the top-level viewport ('grid' and 'graphics' output mixed?)")
    upPath <- structure(paste(parts[(depth - nav + 1L):depth], collapse='::'), class=c('vpPath','path'))
    .rport_grid('up', as.integer(nav))
    if (isTRUE(recording)) .rport_grid('record.up', recorded)
  }
  invisible(upPath)
}
