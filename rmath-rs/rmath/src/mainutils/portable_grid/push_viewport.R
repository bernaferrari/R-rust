function(..., recording=TRUE) { if(!isTRUE(recording)) stop('unrecorded grid operations are not supported');  vs<-list(...); for(v in vs) .rport_grid('push',v); invisible(NULL) }
