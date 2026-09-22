function(colors,...)
{
    ramp <- colorRamp(colors,...)
    function(n) {
        x <- ramp(seq.int(0, 1, length.out = n))
        if (ncol(x) == 4L)
            rgb(x[, 1L], x[, 2L], x[, 3L], x[, 4L], maxColorValue = 255)
        else rgb(x[, 1L], x[, 2L], x[, 3L], maxColorValue = 255)
    }

}
