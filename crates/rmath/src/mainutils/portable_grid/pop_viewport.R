function(n=1, recording=TRUE) {
  if (n < 0) stop('must pop at least one viewport')
  .rport_grid('pop', n)
  invisible(NULL)
}
