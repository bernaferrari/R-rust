function(grobs=TRUE, viewports=FALSE, fullNames=FALSE, recursive=TRUE, print=TRUE, flatten=TRUE) {
  if (!isTRUE(recursive) || !isTRUE(print) || !isTRUE(flatten))
    stop("unsupported grid.ls option")
  lines <- .rport_grid('ls', list(grobs=isTRUE(grobs), viewports=isTRUE(viewports), fullNames=isTRUE(fullNames)))
  if (length(lines)) cat(lines, sep='\n') else cat('\n')
  invisible(lines)
}
