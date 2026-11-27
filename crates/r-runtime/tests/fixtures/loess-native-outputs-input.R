loess_native_outputs <- function() {
    result <- list()
    for (d in 1:4) {
        n <- 20L
        m <- 3L
        i <- seq_len(n)
        x <- matrix(seq(-1, 1, length.out = n), n, 1L)
        if (d >= 2L) x <- cbind(x, sin(i * 1.7))
        if (d >= 3L) x <- cbind(x, cos(i * 0.73))
        if (d >= 4L) x <- cbind(x, sin(i * i * 0.023))
        x <- as.double(x)
        y <- sin(i * 0.3) + i / 50
        weights <- 0.5 + i / n
        robust <- 0.4 + (i %% 3L) / 5
        q <- as.double(matrix(x, n, d)[c(5L, 10L, 16L), , drop = FALSE])
        kd <- max(n, 200L)
        for (surface in c("direct/none", "direct/approximate", "direct/exact",
                          "interpolate/none", "interpolate/1.approx",
                          "interpolate/2.approx", "interpolate/exact")) {
            raw <- .C(stats:::C_loess_raw, as.double(y), x, as.double(weights),
                      as.double(robust), as.integer(d), n, 0.8, 1L,
                      as.integer(d), rep(2L, d), 0L, 0.16, surface,
                      fitted = rep(-11, n), parameter = rep(-13L, 7L),
                      a = rep(-17L, kd), xi = rep(-19, kd),
                      vert = rep(-23, 2L * d), vval = rep(-29, (d + 1L) * kd),
                      diagonal = rep(-31, n), trL = -37, delta1 = -41, delta2 = -43,
                      as.integer(surface == "interpolate/exact"))
            result[[paste(d, surface, sep = ":")]] <-
                raw[c("fitted", "parameter", "a", "xi", "vert", "vval",
                      "diagonal", "trL", "delta1", "delta2")]
        }
        for (family in 0:1) {
            fit <- .C(stats:::C_loess_dfitse, as.double(y), x, q,
                      as.double(weights * robust), as.double(robust),
                      as.integer(family), 0.8, 1L, as.integer(d), rep(2L, d), 0L,
                      as.integer(d), n, m, fit = rep(-47, m), L = rep(-53, n * m))
            result[[paste(d, "direct-se", family, sep = ":")]] <- fit[c("fit", "L")]
        }
        fit <- .C(stats:::C_loess_ise, as.double(y), x, q, as.double(weights),
                  0.8, 1L, as.integer(d), rep(2L, d), 0L, 0.16,
                  as.integer(d), n, m, fit = rep(-59, m), L = rep(-61, n * m))
        result[[paste(d, "interpolate-se", sep = ":")]] <- fit[c("fit", "L")]
    }
    result
}
