function(..., recording=TRUE) {
  vs <- list(...)
  if (!length(vs)) stop('must specify at least one viewport')
  for (v in vs) {
    if (!inherits(v, 'viewport')) stop('only valid to push viewports')
    .rport_grid('push', v)
    if (isTRUE(recording)) .rport_grid('record.push', v$name)
  }
  invisible(NULL)
}
