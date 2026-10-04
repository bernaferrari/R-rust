# GNU build pinned by oracle/r-oracle.json. Its coerce.c preserves real inputs
# and sets imaginary parts to zero, including NA and ordinary NaN.
x <- as.complex(c(NA_real_, NaN, Inf, -Inf, 3.14))
stopifnot(is.na(Re(x)[1L]), !is.nan(Re(x)[1L]),
          is.nan(Re(x)[2L]), identical(Re(x)[3:5], c(Inf, -Inf, 3.14)),
          identical(Im(x), rep(0, 5L)))
cat("real-to-complex NA/NaN/Inf/normal GNU contract: PASS\n")
