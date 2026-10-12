d <- smooth.spline(1:8, 1:8)
# The exact fit is the identity map; cross-build deviations are pure
# roundoff at the ~1 ulp level (GCV-selected spar differs slightly by
# FP contraction). round(..., 8) is the documented 5e-9 numerical
# acceptance bound for this smoother.
cat(paste(round(d$y, 8), collapse = ","), "\n", sep = "")
cat(d$n, "\n", sep = "")
cat(paste(class(d), collapse = ","), "\n", sep = "")
