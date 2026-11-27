# Run from the repository root with the pinned GNU Rscript.
directory <- "crates/r-embed/tests/fixtures"
source(file.path(directory, "loess-native-outputs-input.R"))
saveRDS(loess_native_outputs(), file.path(directory, "gnu-loess-native-outputs.rds"),
        version = 2, compress = FALSE)
n <- 8L
m <- 2L
x <- seq(-1, 1, length.out = n)
y <- sin(seq_len(n) * 0.3)
weights <- seq(0.5, 1.5, length.out = n)
robust <- seq(0.4, 1, length.out = n)
queries <- c(-0.3, 0.4)
raw <- function(surface) .C(stats:::C_loess_raw, y, x, weights, robust,
    1L, n, 0.9, 1L, 1L, 2L, 0L, 0.18, surface,
    fitted = double(n), parameter = integer(7), a = integer(200),
    xi = double(200), vert = double(2), vval = double(400),
    diagonal = rep(-31, n), trL = -37, delta1 = -41, delta2 = -43,
    as.integer(surface == "interpolate/exact"))
direct <- .C(stats:::C_loess_dfitse, y, x, queries, weights * robust,
    robust, 1L, 0.9, 1L, 1L, 2L, 0L, 1L, n, m,
    fit = rep(-47, m), L = rep(-53, n * m))
symmetric <- .C(stats:::C_loess_dfitse, y, x, queries, weights * robust,
    robust, 0L, 0.9, 1L, 1L, 2L, 0L, 1L, n, m,
    fit = rep(-47, m), L = rep(-53, n * m))
interpolate <- .C(stats:::C_loess_ise, y, x, queries, weights,
    0.9, 1L, 1L, 2L, 0L, 0.18, 1L, n, m,
    fit = rep(-59, m), L = rep(-61, n * m))
outputs <- list(x = x, y = y, weights = weights, robust = robust, queries = queries,
    direct_diagonal = raw("direct/approximate")$diagonal,
    interpolate_diagonal = raw("interpolate/1.approx")$diagonal,
    direct_fit = direct$fit, direct_L = direct$L,
    symmetric_fit = symmetric$fit, symmetric_L = symmetric$L,
    interpolate_fit = interpolate$fit, interpolate_L = interpolate$L)
array <- function(values) {
    stopifnot(all(is.finite(values)))
    paste0("[", paste(sprintf("%.17g", values), collapse = ","), "]")
}
json <- paste0("{\n", paste(vapply(names(outputs), function(name)
    paste0('  "', name, '": ', array(outputs[[name]])), ""), collapse = ",\n"), "\n}\n")
cat(json, file = file.path(directory, "gnu-loess-native-kernel.json"))
