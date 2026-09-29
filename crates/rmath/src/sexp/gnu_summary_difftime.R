function(object, digits = getOption("digits"), ...)
{
    x <- summary.default(unclass(object), digits = digits, ...)
    if (m <- match("NAs", names(x), 0L)) {
        NAs <- as.integer(x[m])
        x <- x[-m]
        attr(x, "NAs") <- NAs
    }
    .difftime(x, attr(object, "units"), c("summaryDefault", oldClass(object)))
}
