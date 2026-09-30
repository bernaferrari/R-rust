function(n=1, recording=TRUE) {
  if (n < 0) stop('must pop at least one viewport')
  # GNU records the number it was given. Exact 0 is rewritten to the depth
  # first, so that is what gets recorded. Navigation truncates toward 0, and
  # a positive count that truncates to 0 still pops one viewport.
  path <- .rport_grid('current.path', NULL)
  parts <- if (is.null(path) || !nzchar(path)) character() else strsplit(path, '::', fixed=TRUE)[[1L]]
  depth <- length(parts)
  recorded <- if (n == 0) depth else n
  nav <- if (n == 0) depth else if (n > 0) max(1L, as.integer(n)) else n
  if (nav > 0) .rport_grid('pop', nav)
  if (isTRUE(recording) && nav > 0) .rport_grid('record.pop', recorded)
  invisible(NULL)
}
