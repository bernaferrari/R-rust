# `substring` is base R's alias for `substr` (same handler).
cat("substr-3:", substring("hello", 2, 4), "\n")
cat("substr-2:", substring("hello", 2), "\n")
cat("substr-eq:", identical(substring("hello", 2, 4), substr("hello", 2, 4)), "\n")
