m <- matrix(1:6, nrow = 2)

# Empty subscript slots must pass through as missing arguments (gram.y xxsub0)
print(m[, 1])
print(m[1, ])
print(m[2, ])
print(m[1, 2])
print(m[1, 3])
print(m[1, 1])

# drop=FALSE keeps matrix shape for an otherwise-vector result
print(m[, 1, drop = FALSE])

# Nested recursive indexing
l <- list(list(9))
print(l[[c(1, 1)]])

# Matrix element extraction via double brackets
print(m[[1, 2]])

# data.frame row / column subsetting
df <- data.frame(a = 1:3, b = 4:6)
print(df[1, ])
print(df[, 1])
print(df[2:3, ])

# Empty slots on the assignment side
mm <- matrix(1:6, nrow = 2)
mm[1, ] <- 9
print(mm)
mm2 <- matrix(1:6, nrow = 2)
mm2[, 2] <- 7
print(mm2)
