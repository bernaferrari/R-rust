## Capture with the pinned GNU R executable; dimensions are part of the result.
p <- round(graphics::barplot(c(2, 4, 3), plot = FALSE), 10)
dput(p)
print(identical(p, c(.7, 1.9, 3.1)))
print(identical(p, matrix(c(.7, 1.9, 3.1), ncol = 1)))
