local({
    lowesw <- get("C_lowesw", asNamespace("stats"))
    lowesp <- get("C_lowesp", asNamespace("stats"))
    cases <- list(
        list(c(0, 1, 2), c(1, 2, 3), c(1, 0.945216049382716, 0.7901234567901235)),
        list(rep(0, 4), c(1, 4, 3, 2), rep(1, 4)),
        list(c(2, 1), c(2, 1), c(0.9036732205456486, 0.9754610577655847))
    )
    for (case in cases) {
        n <- length(case[[1L]])
        before <- list(c(case[[1L]], 77), c(as.integer(n), 78L),
                       rep(79, n + 1L), rep(80L, n + 1L))
        actual <- do.call(.Fortran, c(list(lowesw), before))
        expected <- before
        expected[[3L]][seq_len(n)] <- case[[3L]]
        expected[[4L]][seq_len(n)] <- as.integer(case[[2L]])
        stopifnot(isTRUE(all.equal(actual, expected, tolerance = 1e-12)))
    }
    y <- c(3, 1, 4, 1, 5, 9, 2, 6, 77)
    for (zero in c(FALSE, TRUE)) {
        fitted <- if (zero) y else c(2, 2, 3, 2, 4, 8, 1, 5, 78)
        before <- list(c(8L, 81L), y, fitted, c(rep(c(1, 4), 4), 79),
                       c(rep(c(1, 0.5), 4), 80), rep(82L, 9), rep(83, 9))
        actual <- do.call(.Fortran, c(list(lowesp), before))
        expected <- before
        expected[[6L]][1:8] <- if (zero) c(6L, 1L, 7L, 8L, 2L, 5L, 3L, 4L) else c(5L, 1L, 7L, 3L, 2L, 8L, 6L, 4L)
        expected[[7L]][1:8] <- if (zero) rep(NaN, 8) else c(
            3.3598188707226502, 1.3200905646386749, 4.3598188707226502,
            1.3200905646386749, 5.3598188707226502, 8.679909435361326,
            2.3598188707226502, 5.679909435361325)
        stopifnot(isTRUE(all.equal(actual, expected, tolerance = 1e-12)))
    }
    TRUE
})
