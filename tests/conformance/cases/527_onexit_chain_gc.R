# Regression: the remaining on.exit() chain and the closure's return value
# must stay rooted while earlier handlers run (an on.exit expression may
# allocate and call gc()). Upstream context.c R_run_onexits PROTECTs the
# chain head and re-roots the not-yet-run remainder on each iteration; the
# port protects the chain head and routes the remainder through
# RCNTXT.conexit, which the collector traces.
llog <- character(0)
bad <- 0L

inner <- function() {
  on.exit(llog <<- c(llog, "I1"), add = TRUE)
  on.exit({ gc(); invisible(seq(1, 5000, by = 1)); llog <<- c(llog, "I2") }, add = TRUE)
  on.exit({ gc(); llog <<- c(llog, "I3") }, add = TRUE)
  return(seq(1, 5000, by = 1))
}

outer <- function() {
  on.exit({ gc(); invisible(seq(1, 5000, by = 1)); llog <<- c(llog, "O") })
  v <- inner()
  if (length(v) != 5000L || v[1] != 1 || v[5000] != 5000) {
    bad <<- bad + 1L
  }
  "outer-done"
}

for (i in 1:100) {
  llog <<- character(0)
  r <- outer()
  if (paste(llog, collapse = ",") != "I1,I2,I3,O") {
    bad <<- bad + 1L
  }
  if (!is.character(r) || length(r) != 1L || r != "outer-done") {
    bad <<- bad + 1L
  }
}
cat(if (bad == 0L) "onexit chain gc: OK" else paste("onexit chain gc: BROKEN", bad), "\n")
