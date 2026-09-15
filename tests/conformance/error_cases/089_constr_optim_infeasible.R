f <- function(p) (p[1] - 2)^2 + (p[2] + 1)^2
constrOptim(c(2, 0), f, NULL, ui = rbind(c(-1, 0)), ci = -1)
