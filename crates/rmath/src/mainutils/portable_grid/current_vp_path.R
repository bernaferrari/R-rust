function() {
  path <- .rport_grid('current.path', NULL)
  if (is.null(path) || !length(path) || !nzchar(path)) NULL
  else structure(path, class=c('vpPath','path'))
}
