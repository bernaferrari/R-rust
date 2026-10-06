local({
    old <- options(width = 80, max.print = 500)
    on.exit(options(old))
    dd <- as.Date("2012-03-12") + -10000:100
    t1 <- tail(capture.output(dd))
    l6 <- length(capture.output(print(dd, max = 600)))
    t2 <- tail(capture.output(print(dd, max = 500)))
    stopifnot(identical(t1, t2), l6 == 121)
    cat("original Date max workflow:", l6, "\n")
    options(max.print = 3)
    for (x in list(1:7, c(1.5, NA, Inf, -Inf, NaN, 2.5, 3.5),
                   c(TRUE, FALSE, NA, TRUE, FALSE, NA, TRUE),
                   c("a", "b", NA, "d", "e", "f", "g"),
                   c(1+2i, NA_complex_, 2-3i, 4+5i, 6-7i, 8+9i, 10-11i),
                   as.raw(1:7),
                   setNames(1:7, letters[1:7]),
                   as.Date("2012-03-12") + 0:6)) {
        for (m in c(0L, 1L, 3L, 6L, 7L, 8L)) {
            cat("class:", class(x), "max:", m, "\n")
            v <- withVisible(print(x, max = m))
            stopifnot(identical(v$value, x), !v$visible,
                      identical(getOption("max.print"), 3L))
        }
        cat("option restored after explicit max:\n")
        print(x)
    }
    cat("direct print.default and compiled forwarding:\n")
    print.default(1:7, max = 6)
    f <- compiler::cmpfun(function(x, m) print(x, max = m))
    f(1:7, 6)
    f(as.Date("2012-03-12") + 0:6, 6)
})
