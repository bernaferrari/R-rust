function(..., recording=TRUE) {
  # recording stays in the signature so GNU callers can pass FALSE.
  # Navigation still updates the live stack; replay snapshots that stack.
  vs <- list(...)
  if (!length(vs)) stop('must specify at least one viewport')
  for (v in vs) {
    if (!inherits(v, 'viewport')) stop('only valid to push viewports')
    .rport_grid('push', v)
  }
  invisible(NULL)
}
