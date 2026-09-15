cat(tryCatch(plot.spec.coherency(1), error = function(e) conditionMessage(e)), "\n", sep = "")
cat(tryCatch(plot.spec.phase(1), error = function(e) conditionMessage(e)), "\n", sep = "")
