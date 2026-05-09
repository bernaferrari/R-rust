d <- data.frame(a = 1:2, b = 3:4)
d2 <- data.frame(id = row.names(d), a = d$a, b = d$b)
print(paste(names(d2), collapse = "|"))
print(paste(d2$id, collapse = "|"))

d3 <- data.frame(a = d2$a, b = d2$b)
row.names(d3) <- d2$id
print(paste(names(d3), collapse = "|"))
print(paste(row.names(d3), collapse = "|"))

d4 <- data.frame(b = d2$b, id = d2$id, a = d2$a)
print(paste(names(d4), collapse = "|"))
