check_underflow <- function(lower, logged) {
  q <- c(0, 2.5e-312, 1.6e-310, 8e-311, 0.5, 1, NA_real_)
  seen <- character()
  answer <- withCallingHandlers(
    pbeta(q, 1e-15, 1, lower.tail = lower, log.p = logged),
    warning = function(w) {
      stopifnot(identical(conditionCall(w),
                         quote(pbeta(q, 1e-15, 1, lower.tail = lower, log.p = logged))))
      seen <<- c(seen, conditionMessage(w))
      invokeRestart("muffleWarning")
    }
  )
  stopifnot(length(answer) == length(q), is.na(answer[7]),
            all(is.finite(answer[2:5])), length(seen) == 2L)
  if (lower) {
    stopifnot(answer[1] == if (logged) -Inf else 0,
              answer[6] == if (logged) 0 else 1)
  } else {
    stopifnot(answer[1] == if (logged) 0 else 1,
              answer[6] == if (logged) -Inf else 0)
  }
  stopifnot(answer[3] != answer[4])
  cat(paste(lower, logged), "\n", sep = "")
  cat(seen, sep = "\n")
}

check_underflow(TRUE, FALSE)
check_underflow(FALSE, FALSE)
check_underflow(TRUE, TRUE)
check_underflow(FALSE, TRUE)
