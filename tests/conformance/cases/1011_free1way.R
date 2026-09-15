cat(tryCatch(free1way(), error = function(e) conditionMessage(e)), "\n", sep = "")
cat(tryCatch(power.free1way.test(), error = function(e) conditionMessage(e)), "\n", sep = "")
