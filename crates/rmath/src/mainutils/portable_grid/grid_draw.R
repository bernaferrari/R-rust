function(x,recording=TRUE) {
    if(is.null(x)) return(invisible(NULL))
    if(!inherits(x,'grob')) stop('grid.draw requires a grob')
    # A grob vp stays on the viewport tree (up, not pop) so downViewport can
    # find it. The listing prints that vp from the grob, so this push is not
    # recorded a second time. A gTree gp viewport is only a drawing parent.
    # on.exit(add=TRUE) runs in registration order, so the later gp push is
    # popped before the grob vp is moved up.
    # Character and vpPath slots navigate; only a viewport object is pushed.
    vp.depth <- 0L
    if(!is.null(x$vp)) {
      if (inherits(x$vp, 'viewport')) {
        pushViewport(x$vp, recording=FALSE)
        vp.depth <- 1L
      } else {
        downViewport(x$vp, strict=TRUE, recording=FALSE)
        vp.depth <- length(strsplit(as.character(x$vp), '::', fixed=TRUE)[[1L]])
      }
    }
    if(inherits(x,'gTree')) {
        if(!is.null(x$gp)) {
            pushViewport(viewport(gp=x$gp), recording=FALSE)
            on.exit(popViewport(1, recording=FALSE), add=TRUE)
        }
        # Children are drawn, not recorded; the gTree is one display-list entry.
        for(child in x$children) grid.draw(child, recording=FALSE)
    } else .rport_grid('draw',x)
    if(vp.depth > 0L) on.exit(upViewport(vp.depth, recording=FALSE), add=TRUE)
    if(isTRUE(recording)) .rport_grid('record', x)
    invisible(NULL)
}
