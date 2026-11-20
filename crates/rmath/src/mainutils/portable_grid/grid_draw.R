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
        .rport_grid('grob.push', x$vp)
        vp.depth <- 1L
      } else {
        path <- unlist(strsplit(as.character(x$vp), '::', fixed=TRUE), use.names=FALSE)
        if (!length(path) || anyNA(path) || any(!nzchar(path)))
            stop('a viewport path must contain at least one viewport name')
        .rport_grid('grob.down', list(name=paste(path, collapse='::'), strict=TRUE))
        vp.depth <- length(path)
      }
    }
    if(inherits(x,'gTree')) {
        if(!is.null(x$gp)) {
            .rport_grid('grob.push', viewport(gp=x$gp))
            on.exit(.rport_grid('grob.pop', 1L), add=TRUE)
        }
        # Children are drawn, not recorded; the gTree is one display-list entry.
        for(child in x$children) grid.draw(child, recording=FALSE)
    } else .rport_grid('draw',x)
    if(vp.depth > 0L) on.exit(.rport_grid('grob.up', vp.depth), add=TRUE)
    if(isTRUE(recording)) .rport_grid('record', x)
    invisible(NULL)
}
