function(fullNames=FALSE) {
  lines <- .rport_grid('ls', list(fullNames=isTRUE(fullNames)))
  if (length(lines)) cat(lines, sep='\n') else cat('\n')
  invisible(lines)
}
