values <- list(TRUE, "1", 1+0i, numeric(0), c(1,2), list(1), as.raw(1), NA_integer_, NA_real_, 1L, 1)
for (mode in c("optimize", "uniroot")) {
  for (i in seq_along(values)) {
    n <- 0L
    f <- function(x) { n <<- n+1L; if (n == 1L) values[[i]] else x }
    status <- tryCatch({
      if (mode == "optimize") optimize(f, c(-1,1)) else uniroot(f, c(-1,1), f.lower=-1, f.upper=1)
      "ok"
    }, warning=function(w) paste0("warning:", conditionMessage(w)),
       error=function(e) paste0("error:", conditionMessage(e)))
    cat(mode, "|", i, "|", status, "\n", sep="")
  }
}
