function(x, sep = "", base = list(. = LETTERS), unique = TRUE,
         use.names = FALSE)
{
    dx <- dim(x)
    dnx <- dimnames(x)
    if(new <- is.null(dnx))
        dnx <- vector("list", length(dx))
    k <- length(M <- lengths(base))
    for(i in which(vapply(dnx, is.null, NA))) {
        ii <- 1L+(i-1L) %% k
        ss <- seq_len(dx[i]) - 1L
        bi <- base[[ii]][1L+ (ss %% M[ii])]
        dnx[[i]] <- if(unique) make.unique(bi, sep = sep) else bi
        new <- TRUE
    }
    if(use.names && !is.null(nbase <- names(base)) &&
       (is.null(ndnx <- names(dnx)) || length(i <- which(!nzchar(ndnx))))) {
        nbi <- if(is.null(ndnx)) rep_len(nbase, length(dx))
               else nbase[1L + (i-1L) %% k]
        if(unique)
            nbi <- make.unique(nbi, sep = sep)
        names(dnx) <- if(is.null(ndnx)) nbi
                      else `[<-`(ndnx, i, nbi)
        new <- TRUE
    }
    if(new) dimnames(x) <- dnx
    x
}
