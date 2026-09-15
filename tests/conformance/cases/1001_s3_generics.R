cat(tryCatch(preplot(1), error = function(e) conditionMessage(e)), "\n", sep = "")
cat(tryCatch(profile(1), error = function(e) conditionMessage(e)), "\n", sep = "")
cat(tryCatch(tsdiag(1), error = function(e) conditionMessage(e)), "\n", sep = "")
