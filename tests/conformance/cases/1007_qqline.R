cat(tryCatch(qqline(c(1.1, 1.9, 3.2, 3.8, 5.1)), error = function(e) conditionMessage(e)), "\n", sep = "")
