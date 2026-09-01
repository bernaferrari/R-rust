s <- 0L
for (i in c(1L, 2L, 3L, 4L)) s <- s + i
cat(s, i, "\n")

i <- 99L
for (i in seq_len(0L)) s <- -1L
cat(is.null(i), s, "\n")

j <- 0L
while (j < 100L) j <- j + 1L
cat(j, "\n")
