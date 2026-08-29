# dpq vectorized recycling: parameter formals recycle against the main
# argument per stock SETUP_MathN. Expected values are stock R's to 15
# significant digits; stopifnot fails the case on any divergence.
eq <- function(actual, expected) {
  stopifnot(length(actual) == length(expected))
  for (i in 1:length(expected)) {
    d <- abs(actual[i] - expected[i])
    if (is.na(d) || d > 1e-9 * (1 + abs(expected[i]))) stop("mismatch at ", i)
  }
}
eq(dnorm(c(1, 2), mean = c(0, 10)), c(0.24197072451914337, 5.0522710835368927e-15))
eq(dnorm(c(1, 2), mean = c(0, 10, 20)), c(0.24197072451914337, 5.0522710835368927e-15, 1.6246360367736081e-79))
eq(pbinom(c(1, 2), 10, c(.2, .5)), c(0.37580963840000031, 0.054687499999999972))
eq(qchisq(c(.1, .9), c(1, 2)), c(0.015790774093431229, 4.6051701859880918))
eq(pbeta(c(.2, .6, .9), c(1, 2), c(3, 1, 2)), c(0.48799999999999977, 0.35999999999999999, 0.98999999999999999))
eq(dhyper(c(1, 2, 3), c(4, 5), c(6, 7, 8), c(3, 4)), c(0.5, 0.42424242424242431, 0.018181818181818181))
eq(dnbinom(c(1, 2, 3), size = c(3, 4), mu = c(2, 5, 7)), c(0.25920000000000004, 0.1204272910821709, 0.092609999999999998))
eq(dgamma(c(1, 2, 3), c(1, 2), rate = c(1, 4)), c(0.36787944117144233, 0.010734804092880379, 0.049787068367863944))
eq(dt(c(1, 2, 3), df = c(1, 2), ncp = c(.5, 1)), c(0.2250114947643107, 0.17910974594042361, 0.053595655804235504))
eq(dnorm(c(1, 2), log = c(TRUE, FALSE)), c(-1.4189385332046727, -2.9189385332046727))
eq(qnorm(c(.1, .9), lower.tail = c(FALSE, FALSE)), c(1.2815515655446006, -1.2815515655446006))
eq(punif(c(.25, .5, .75), min = c(0, .5), max = c(1, 2)), c(0.25, 0, 0.75))
eq(dsignrank(c(1, 2), n = c(4, 5, 6)), c(0.0625, 0.03125, 0.015625000000000007))
eq(dwilcox(c(1, 2), m = c(3, 4), n = c(5, 6, 7)), c(0.017857142857142856, 0.0095238095238095247, 0.0083333333333333332))
# zero-length operand gives a zero-length result (trunk, not the old
# "invalid arguments" error)
eq(length(dnorm(1, mean = numeric(0))), 0)
eq(length(pbinom(c(1, 2, 3), size = numeric(0), prob = 0.5)), 0)
# names propagate from the matching-length operand
stopifnot(all(names(dnorm(c(a = 1, b = 2), 0)) == c("a", "b")))
stopifnot(all(names(dnorm(c(a = 1), mean = c(x = 0, y = 10))) == c("x", "y")))
# log(x, base) recycles both arguments
eq(log(c(8, 100), base = c(2, 10)), c(3, 2))
cat("dpq recycling ok\n")
