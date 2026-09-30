function() {
  lines <- .rport_grid('ls', NULL)
  if (length(lines)) cat(lines, sep='\n') else cat('\n')
  invisible(lines)
}
