# substr/substring recycle x/start/stop to the COMMON length
# (max of the three) per upstream substr.c — vector bounds with a scalar
# string yield a vector result (substring over gregexpr's per-match
# positions was silently dropping all but the first position).
cat("vec-both:", substring("abcde", c(1, 2), c(2, 3)), "\n")
cat("vec-start:", substring("abcde", c(1, 2), 3), "\n")
cat("vec-stop:", substring("abcde", 2, c(3, 4)), "\n")
cat("scalar:", substring("hello", 2, 4), "\n")
cat("substr-vec:", substr("abcde", c(1, 2), c(2, 3)), "\n")
cat("gregexpr-pair:", substring("{{#s}}x{{/s}}", c(1, 7), c(5, 11)), "\n")
