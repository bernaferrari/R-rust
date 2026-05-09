h <- head(c(a = "x", b = "y", c = NA), 2)
print(names(h))
print(h)

t <- tail(c(a = "x", b = "y", c = NA), 2)
print(names(t))
print(t[1])
print(unname(is.na(t[2])))

lh <- head(list(a = 1, b = "x", c = TRUE), 2)
print(names(lh))
print(lh[[2]])

lt <- tail(list(a = 1, b = "x", c = TRUE), 2)
print(names(lt))
print(lt[[1]])
