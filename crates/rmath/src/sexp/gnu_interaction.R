function(..., drop = FALSE, sep = ".", lex.order = FALSE)
{
    args <- list(...)
    narg <- length(args)
    if (narg < 1L)
	stop("No factors specified")
    if (narg == 1L && is.list(args[[1L]])) {
	args <- args[[1L]]
	narg <- length(args)
    }
    for(i in narg:1L) {
        x <- as.factor(args[[i]])[, drop = drop]
        ax <- as.integer(x) - 1L
        lx <- levels(x)
        if(i == narg) {
            ay <- ax
            ly <- lx
        } else {
            nx <- length(lx)
            ny <- length(ly)
            if(lex.order) {
                ay <- ay + as.numeric(ny) * ax
                if(drop) {
                    az <- sort(unique(ay))
                    ly <- paste(lx[az %/% ny + 1L], ly[az %% ny + 1L],
                                sep = sep)
                    ay <- match(ay, az) - 1L
                } else {
                    ly <- paste(rep(lx, each = ny), rep(ly, nx),
                                sep = sep)
                }
            } else {
                ay <- ay * as.numeric(nx) + ax
                if(drop) {
                    az <- sort(unique(ay))
                    ly <- paste(lx[az %% nx + 1L], ly[az %/% nx + 1L],
                                sep = sep)
                    ay <- match(ay, az) - 1L
                } else {
                    ly <- paste(rep(lx, ny), rep(ly, each = nx),
                                sep = sep)
                }
            }
            while(j <- anyDuplicated(ly)) {
                i <- match(ly[j], ly)
                ly <- ly[-j]
                j <- j - 1L
                ay[ay == j] <- i - 1L
                ay[ay > j] <- ay[ay > j] - 1L
            }
        }
    }
    structure(as.integer(ay + 1L), levels = ly, class = "factor")
}
