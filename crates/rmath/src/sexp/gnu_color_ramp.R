function(colors, bias = 1, space = c("rgb","Lab"),
                    interpolate = c("linear","spline"), alpha = FALSE)
{
    if (bias <= 0) stop("'bias' must be positive")
    if (!missing(space) && alpha)
        stop("'alpha' must be false if 'space' is specified")

    colors <- t(col2rgb(colors, alpha = alpha)/255)
    space <- match.arg(space)
    interpolate <- match.arg(interpolate)

    if (space == "Lab")
        colors <- convertColor(colors, from = "sRGB", to = "Lab")


    interpolate <- switch(interpolate,
                          linear = stats::approxfun,
                          spline = stats::splinefun)

    if((nc <- nrow(colors)) == 1L) {
        colors <- colors[c(1L, 1L) ,]
        nc <- 2L
    }
    x <- seq.int(0, 1, length.out = nc)^bias
    palette <- c(interpolate(x, colors[, 1L]),
                 interpolate(x, colors[, 2L]),
                 interpolate(x, colors[, 3L]),
                 if(alpha) interpolate(x, colors[, 4L]))

    roundcolor <- function(rgb) ## careful to preserve matrix:
	pmax(pmin(rgb, 1), 0)

    if (space == "Lab")
        function(x)
            roundcolor(convertColor(cbind(palette[[1L]](x),
                                          palette[[2L]](x),
                                          palette[[3L]](x),
                                          if(alpha) palette[[4L]](x)),
                                    from = "Lab", to = "sRGB"))*255
    else
        function(x)
            roundcolor(cbind(palette[[1L]](x),
                             palette[[2L]](x),
                             palette[[3L]](x),
                             if(alpha) palette[[4L]](x)))*255
}
