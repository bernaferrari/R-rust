cat(tryCatch(plclust(), error = function(e) conditionMessage(e)), "\n", sep = "")
