# Actual pinned GNU conversion controls; internal isVectorizable categories are
# additionally taken from its pinned Rinlinedfuns.h, lines1006-1022.
stopifnot(identical(as.integer(pairlist(1L,2L)),c(1L,2L)),
          identical(as.logical(pairlist(logical(),TRUE)),c(NA,TRUE)),
          identical(as.integer(list(integer(),2L)),c(NA_integer_,2L)),
          inherits(try(as.integer(expression(1L)),silent=TRUE),"try-error"))
cat("pairlist/list empty scalars and expression rejection: PASS\n")
