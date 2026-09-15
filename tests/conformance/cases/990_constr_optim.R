f <- function(p) (p[1] - 2)^2 + (p[2] + 1)^2
cat(paste(round(constrOptim(c(0, 0), f, NULL, ui = rbind(c(-1, 0)), ci = -1)$par, 4), collapse = ","), "\n", sep = "")
