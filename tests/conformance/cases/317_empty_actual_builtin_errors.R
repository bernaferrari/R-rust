print(tryCatch(list(, 1), error = function(e) conditionMessage(e)))
print(tryCatch(sum(1, ), error = function(e) conditionMessage(e)))
print(tryCatch(c(, 1), error = function(e) conditionMessage(e)))
