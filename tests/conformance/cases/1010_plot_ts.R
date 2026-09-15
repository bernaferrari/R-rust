cat(tryCatch(as.ts(list()), error = function(e) conditionMessage(e)), "\n", sep = "")
cat(tryCatch(plot.ts(list()), error = function(e) conditionMessage(e)), "\n", sep = "")
cat(tryCatch(ts.plot(), error = function(e) conditionMessage(e)), "\n", sep = "")
cat(is.null(ts.plot(1:5)), "\n", sep = "")
