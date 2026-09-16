cat(paste(names(c(a = pi, b = 1, d = 1:4)), collapse = ","), "\n", sep = "")
ff <- gl(2, 3) : gl(3, 2)
cat(all(levels(ff) == t(outer(1:2, 1:3, paste, sep = ":"))), "\n", sep = "")
x <- NULL
x$x1 <- 1:10
x$x2 <- 0:9
dx <- as.data.frame(x)
cat(paste(dim(dx), collapse = ","), "\n", sep = "")
xdf <- data.frame(a = 1:3)
x30 <- xdf[, -1]
m30 <- as.matrix(x30)
cat(typeof(m30), "\n", sep = "")
cat(paste(dim(data.frame(m30)), collapse = ","), "\n", sep = "")
cat(paste(dim(cbind(xdf, x30)), collapse = ","), "\n", sep = "")
m <- cbind(a = 1:2, b = c(R = 10, S = 11))
cat(paste(sapply(dimnames(m), length), collapse = ","), "\n", sep = "")
x <- c(3:1, 6, 4, 3, NA, 5, 0, NA)
rx <- rank(x)
rxK <- rank(x, na.last = "keep")
cat(all(rx[rx <= 8] == na.omit(rxK)), "\n", sep = "")
cat(all(rank(x, na.last = NA) == na.omit(rxK)), "\n", sep = "")
