function(x, m, invert = FALSE)
{
    if(length(x) != length(m))
        stop(gettextf("%s and %s must have the same length",
                      sQuote("x"), sQuote("m")),
             domain = NA)

    ili <- is.list(m)

    itype <- "chars"
    useBytes <- if(ili)
        any(unlist(lapply(m, attr, "index.type")) == "bytes")
    else
        any(attr(m, "index.type") == "bytes")
    if(useBytes) {
        itype <- Encoding(x) <- "bytes"
    }

    if(!ili && isFALSE(invert)) {
        so <- m[ind <- (!is.na(m) & (m > -1L))]
        eo <- so + attr(m, "match.length")[ind] - 1L
        return(substring(x[ind], so, eo))
    }

    y <- if(is.na(invert)) {
        Map(function(u, so, ml) {
                if((n <- length(so)) == 1L) {
                    if(is.na(so) )
                        return(NA_character_)
                    else if(so == -1L)
                        return(u)
                }
                eo <- so + ml - 1L
                if(n > 1L) {
                    if(any(eo[-n] >= so[-1L]))
                        stop(gettextf("need non-overlapping matches for %s",
                                      sQuote("invert = NA")),
                             domain = NA)
                }
                beg <- c(1L, c(rbind(so, eo + 1L)))
                end <- c(c(rbind(so - 1L, eo)), nchar(u, itype))
                substring(u, beg, end)
            },
            x, m,
            if(ili)
                lapply(m, attr, "match.length")
            else
                attr(m, "match.length"),
            USE.NAMES = FALSE)
    } else if(invert) {
        Map(function(u, so, ml) {
                if((n <- length(so)) == 1L) {
                    if(is.na(so) )
                        return(NA_character_)
                    else if(so == -1L)
                        return(u)
                }
                beg <- if(n > 1L) {
                    eo <- so + ml - 1L
                    if(any(eo[-n] >= so[-1L]))
                        stop(gettextf("need non-overlapping matches for %s",
                                      sQuote("invert = TRUE")),
                             domain = NA)
                    c(1L, eo + 1L)
                } else {
                    c(1L, so + ml)
                }
                end <- c(so - 1L, nchar(u, itype))
                substring(u, beg, end)
            },
            x, m,
            if(ili)
                lapply(m, attr, "match.length")
            else
                attr(m, "match.length"),
            USE.NAMES = FALSE)
    } else {
        Map(function(u, so, ml) {
                if(length(so) == 1L) {
                    if(is.na(so) || (so == -1L))
                        return(character())
                }
                tmp <- substring(u, so, so + ml - 1L)
                dim(tmp) <- dim(so)
                dimnames(tmp) <- dimnames(so)
                names(tmp) <- names(so)
                tmp
            },
            x, m,
            lapply(m, attr, "match.length"),
            USE.NAMES = FALSE)
    }

    names(y) <- names(x)
    y
}
