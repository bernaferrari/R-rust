function(x,recording=TRUE) {
    if(is.null(x)) return(invisible(NULL))
    if(!inherits(x,'grob')) stop('grid.draw requires a grob')
    pushed <- 0L
    if(!is.null(x$vp)) { pushViewport(x$vp); pushed <- pushed+1L }
    on.exit(if(pushed>0L) popViewport(pushed))
    if(inherits(x,'gTree')) {
        if(!is.null(x$gp)) { pushViewport(viewport(gp=x$gp)); pushed <- pushed+1L }
        # Children are drawn, not recorded; the gTree is one display-list entry.
        for(child in x$children) grid.draw(child, recording=FALSE)
    } else .rport_grid('draw',x)
    if(isTRUE(recording)) .rport_grid('record', x)
    invisible(NULL)
}
