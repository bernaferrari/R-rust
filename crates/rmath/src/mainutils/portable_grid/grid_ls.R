function(x=NULL, grobs=TRUE, viewports=FALSE, fullNames=FALSE, recursive=TRUE, print=TRUE, flatten=TRUE) {
  if (!isTRUE(recursive) || !isTRUE(print) || !isTRUE(flatten))
    stop("unsupported grid.ls option")
  payload <- list(grobs=isTRUE(grobs), viewports=isTRUE(viewports), fullNames=isTRUE(fullNames))
  lines <- if (is.null(x)) .rport_grid('ls', payload) else {
    payload$x <- x
    .rport_grid('ls.grob', payload)
  }
  if (length(lines)) cat(lines, sep='\n') else cat('\n')
  invisible(lines)
}
