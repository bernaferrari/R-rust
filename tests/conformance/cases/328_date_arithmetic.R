d <- as.Date("2020-02-03")
print(d + 1)
print(1 + d)
print(d - 1)

delta <- d - as.Date("2020-02-01")
print(delta)
print(class(delta))
print(attr(delta, "units"))

print(tryCatch(d + d, error = function(e) conditionMessage(e)))
print(tryCatch(2 - d, error = function(e) conditionMessage(e)))
print(tryCatch(d * 2, error = function(e) conditionMessage(e)))
