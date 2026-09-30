function(n=1, recording=TRUE) {
  if (!isTRUE(recording)) stop('unrecorded grid operations are not supported')
  if (n < 0) stop('must navigate up at least one viewport')
  path <- .rport_grid('current.path', NULL)
  parts <- if (is.null(path) || !nzchar(path)) character() else strsplit(path, '::', fixed=TRUE)[[1L]]
  depth <- length(parts)
  if (n == 0) n <- depth
  upPath <- NULL
  if (n > 0) {
    if (n > depth) stop("cannot pop the top-level viewport ('grid' and 'graphics' output mixed?)")
    if (n != as.integer(n)) stop('invalid viewport up count')
    upPath <- structure(paste(parts[(depth - n + 1L):depth], collapse='::'), class=c('vpPath','path'))
    .rport_grid('up', as.integer(n))
  }
  invisible(upPath)
}
