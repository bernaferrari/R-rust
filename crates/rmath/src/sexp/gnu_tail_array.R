function(x, n = 6L, keepnums = TRUE, addrownums, ...)
{
    if(!missing(addrownums)) {
        .Deprecated(msg = gettext("tail(., addrownums = V) is deprecated.\nUse ",
                                  "tail(., keepnums = V) instead.\n"))
        if(missing(keepnums))
            keepnums <- addrownums
    }

    .checkHT(n, d <- dim(x))
    ## non-specified dimensions (ie length(n) < length(d) or n[i] is NA) will stay missing / empty:
    ii <- which(!is.na(n[seq_along(d)]))
    sel <- lapply(ii, function(i) {
        di <- d[i]
        ni <- n[i]
        seq.int(to = di, ## handle negative n's; result is *integer* iff ds[] is
                length.out = if(ni < 0L) max(di + ni, 0L) else min(ni, di))
        })
    args <- rep(alist(x, , drop = FALSE), c(1L, length(d), 1L))
    args[1L + ii] <- sel
    ans <- do.call(`[`, args)
    if (keepnums && length(d) > 1L) {
        jj <- if(!is.null(adnms <- dimnames(ans)[ii]))
                  which(vapply(adnms, is.null, NA)) else seq_along(ii)
        ## For data.frames dimnames(.) never has null elements
        ## but dimnames(.)[numeric()]<-list() converts default
        ## row.names from INTSXP to AltString STRSXP, so avoid it.
        if(length(jj) > 0) {
            ## jj are indices in sel/ii
            dimnames(ans)[ii[jj]] <- lapply(jj,
                                            function(k) {
                ## No formatting for cols b/c padding not constant when
                ## reprinted across higher dimensions
                ## 1 is rownames, pseudo-col so format [.,]
                ## 2 is colnames, pseudo-row so straight [,.]
                ## >2, return correct/orig indices
                if((dnum <- ii[k]) == 1L)
                    format(sprintf("[%d,]", sel[[k]]),
                           justify = "right")
                else if(dnum == 2L)
                    sprintf("[,%d]", sel[[k]])
                else ## dnum > 2
                    sel[[k]]
            })
        }
    }
    ans
}
