# Regression: a closure's return value must survive gc() called from
# on.exit() handlers (both the explicit return() path and the tail-value
# path). Upstream protects the value across do_onexit via cntxt.returnValue;
# the port roots it in RCNTXT.returnValue, which the collector traces.
mk <- function(n) seq(1, n, by = 1)
churn <- function(n) invisible(rep(7.5, n))

f_return <- function(n) {
  on.exit({ gc(); churn(n); gc() })
  return(mk(n))
}

f_tail <- function(n) {
  on.exit({ gc(); churn(n); gc() })
  mk(n)
}

bad <- 0L
n <- 20000L
for (i in 1:100) {
  v <- f_return(n)
  if (length(v) != n || v[1] != 1 || v[n %/% 2] != n %/% 2 || v[n] != n) {
    bad <- bad + 1L
  }
  w <- f_tail(n)
  if (length(w) != n || w[1] != 1 || w[n %/% 2] != n %/% 2 || w[n] != n) {
    bad <- bad + 1L
  }
}
cat(if (bad == 0L) "onexit gc return: OK" else paste("onexit gc return: CORRUPTED", bad), "\n")
