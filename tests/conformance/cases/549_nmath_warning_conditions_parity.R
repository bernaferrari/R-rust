# Mathlib (nmath) warnings route through R's warning machinery like stock:
# dpq.h's "non-integer x = %f" (R_D_nonint_check) and nmath.h's ML_WARNING
# messages are deferred-collected with call attribution under the default
# warn=0, suppressed by suppressWarnings(), while ML_DOMAIN stays
# stock-silent (the NaN surfaces through the wrapper's "NaNs produced").
dhyper(1.5, 10, 10, 5)
dpois(1.5, 10)
dbinom(1.5, 10, 0.5)
dgeom(1.5, 0.25)
dnbinom(1.5, 10, 0.25)
pbinom(1, 2.5, 0.5)
besselJ(-1, 0.5)
print(suppressWarnings(dhyper(1.5, 10, 10, 5)))

# ML_DOMAIN (ML_WARN_return_NAN) is silent from nmath itself; the wrapper's
# "NaNs produced" is the only stock-visible trace
print(qsignrank(1, 5, log.p = TRUE))
