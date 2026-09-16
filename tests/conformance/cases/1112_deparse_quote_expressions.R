bx <- quote({
    1 + 1
})
d <- deparse(bx, control = "all")
cat(d[1], "\n", sep = "")
cat(d[length(d)], "\n", sep = "")
ob2 <- eval(parse(text = d))
cat(identical(bx, ob2), "\n")
