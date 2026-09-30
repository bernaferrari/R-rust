function(..., recording=TRUE) {
  if (!isTRUE(recording)) stop('unrecorded grid operations are not supported')
  vs <- list(...)
  if (!length(vs)) stop('must specify at least one viewport')
  for (v in vs) {
    if (!inherits(v, 'viewport')) stop('only valid to push viewports')
    .rport_grid('push', v)
  }
  invisible(NULL)
}
