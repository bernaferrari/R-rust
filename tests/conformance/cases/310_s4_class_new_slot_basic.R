setClass("A", slots = c(stuff = "numeric"))
a <- new("A", stuff = c(1, 2))

print(is(a, "A"))
print(slot(a, "stuff"))
print(as.character(class(a)))
