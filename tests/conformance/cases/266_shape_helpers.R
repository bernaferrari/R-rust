print(NROW(NULL))
print(NCOL(NULL))
print(NROW(1:3))
print(NCOL(1:3))

df <- data.frame(a = 1:3, b = 4:6)
print(NROW(df))
print(NCOL(df))

print(is.array(1:4))
print(is.array(matrix(1:4, 2)))

x <- 1:4
dim(x) <- c(2, 2)
print(is.array(x))
