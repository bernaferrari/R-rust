function(n=1, recording=TRUE) {
  if (!isTRUE(recording)) stop('unrecorded grid operations are not supported')
  if (n < 0) stop('must pop at least one viewport')
  .rport_grid('pop', n)
  invisible(NULL)
}
