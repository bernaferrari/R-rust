cat(tryCatch(biplot(1), error = function(e) conditionMessage(e)), "\n", sep = "")
cat(tryCatch(screeplot(1), error = function(e) conditionMessage(e)), "\n", sep = "")
