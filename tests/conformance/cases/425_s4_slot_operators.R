setClass("P", slots = c(x = "numeric", y = "character"))
p <- new("P", x = 1, y = "a")

print(p@x)
p@y <- "b"
print(p@y)
q <- `@<-`(p, "x", 2)
print(q@x)
