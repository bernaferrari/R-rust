function(n=1, recording=TRUE) {
  if (n < 0) stop('must pop at least one viewport')
  # Same truncation as upViewport. Exact 0 stays 0, which pops the whole stack.
  if (n > 0) n <- max(1L, as.integer(n))
  .rport_grid('pop', n)
  invisible(NULL)
}
