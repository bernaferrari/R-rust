# Run after compiling plotmath_font_oracle.c with R CMD SHLIB.
# RPORT_DEJAVU_DIR must point at the graphics-engine assets directory.
# The shell wrapper sets this and builds the test-only metric device.
args <- commandArgs(trailingOnly = TRUE)
if (length(args) != 1) stop("usage: Rscript plotmath_font_oracle.R oracle.so")
dyn.load(args[[1]])
corpus <- list(
  alpha = expression(alpha),
  fraction = expression(frac(alpha[1]^2, sqrt(beta))),
  radical = expression(sqrt(x)),
  delimiters = expression(bgroup("(", alpha[1]^2, ")")),
  sum = expression(sum(i == 1, n, i^2)),
  integral = expression(integral(f(x) * dx))
)
for (size in c(6, 12, 24)) {
  for (name in names(corpus)) {
    value <- .Call("plotmath_font_oracle", corpus[[name]], size)
    cat(sprintf("%s %d %.9f %.9f\n", name, size, value[[1]], value[[2]]))
  }
}
