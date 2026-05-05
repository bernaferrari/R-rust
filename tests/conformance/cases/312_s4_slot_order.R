setClass("Order", slots = c(z = "numeric", a = "numeric"))
x <- new("Order", z = 1, a = 2)

print(slotNames("Order"))
print(slotNames(x))
print(slot(x, "z"))
print(slot(x, "a"))
