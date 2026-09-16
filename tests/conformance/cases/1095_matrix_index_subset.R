m <- matrix(1:25, ncol = 5, dimnames = list(letters[1:5], LETTERS[1:5]))
si <- matrix(c(1, 1, 2, 3, 3, 4), ncol = 2, byrow = TRUE)
ss <- matrix(c("a", "A", "b", "C", "c", "D"), ncol = 2, byrow = TRUE)
stopifnot(identical(m[si], m[ss]))
stopifnot(identical(c(1L, 12L, 18L), m[ss]))
cat(m[si], "\n", sep = " ")
ssna <- ss
ssna[2, 2] <- NA
cat(is.na(m[ssna])[2], "\n", sep = "")
