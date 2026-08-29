# Mersenne-Twister default engine (stock RNG.c parity flagship): the
# set.seed 69069 seed chain + FixupSeeds, the 626-word .Random.seed layout
# (kinds word, mti, mt[0..623]), inversion normals, sample() consuming the
# same stream through R_unif_index rejection, kind switching with per-kind
# state layouts, set.seed kind passthrough, and .Random.seed round-tripping.
cat(paste(RNGkind(), collapse = "|"), "\n")
set.seed(1)
print(runif(3))
set.seed(1)
print(rnorm(2))
set.seed(1)
print(length(.Random.seed))
print(.Random.seed[1:6])
set.seed(42)
print(runif(1))
# Default-digits print exercises the common-decimals vector encoding
# (re-encode at the widest per-element decimal count, zero-pad shorter).
set.seed(5)
print(rweibull(2, 1.5))
set.seed(5)
print(rexp(2))
# sample() draws through the same session stream.
set.seed(7)
print(sample(1:10, 5))
set.seed(7)
print(sample(10, 5, replace = TRUE))
set.seed(7)
print(sample(c("a", "b", "c"), 8, replace = TRUE, prob = c(0.5, 0.25, 0.25)))
set.seed(1)
print(sample(1:10, 5))
print(.Random.seed[2])
set.seed(1)
invisible(sample(1:10, 5))
print(runif(1))
set.seed(9)
print(runif(2))
print(length(.Random.seed))
print(.Random.seed[1:5])
new <- RNGkind("Mersenne-Twister")
cat(paste(new, collapse = "|"), "\n")
# set.seed kind passthrough and the "default" reset.
set.seed(42, kind = "Wichmann-Hill", normal.kind = "default")
cat(paste(RNGkind(), collapse = "|"), "\n")
prev <- RNGkind("default", normal.kind = "default")
cat(paste(prev, collapse = "|"), "\n")
print(rnorm(1))
# .Random.seed round-trip: rewinding reproduces the same stream head.
set.seed(1)
s <- .Random.seed
u1 <- runif(1)
.Random.seed <- s
u2 <- runif(1)
stopifnot(identical(u1, u2))
cat("roundtrip ok\n")
