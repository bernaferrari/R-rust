function(x) {
    if (!is.unit(x)) stop("length.unit requires a unit object")
    length(x$value)
}
