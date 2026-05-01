x <- 1:4
tsp(x) <- 1:3
print(tsp(x))
print(typeof(tsp(x)))

tsp(x) <- NULL
print(tsp(x))
print(attributes(x))

y <- 1:4
tsp(y) <- c(1, 4, NA)
print(tsp(y))

z <- 1:4
tsp(z) <- c(NaN, 4, 1)
print(tsp(z))

print(tryCatch({
    bad <- 1:4
    tsp(bad) <- c(1, 2)
    bad
}, error = function(e) conditionMessage(e)))
print(tryCatch({
    bad <- 1:4
    tsp(bad) <- c(1, 4, 0)
    bad
}, error = function(e) conditionMessage(e)))
print(tryCatch({
    bad <- 1:4
    tsp(bad) <- c(4, 1, 1)
    bad
}, error = function(e) conditionMessage(e)))
print(tryCatch({
    bad <- 1:4
    tsp(bad) <- c("1", "4", "1")
    bad
}, error = function(e) conditionMessage(e)))
