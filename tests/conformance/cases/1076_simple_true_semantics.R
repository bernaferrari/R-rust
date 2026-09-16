cat(typeof(1e-3L), "\n", sep = "")
cat(inherits(try(parse(text = "12iL"), silent = TRUE), "try-error"), "\n", sep = "")
f1 <- y1 ~ x1
f2 <- y2 ~ x2
f2[2] <- f1[2]
cat(deparse(f2), "\n", sep = "")
cat(identical(as.list(as.list), alist(x = , ... = , UseMethod("as.list"))), "\n", sep = "")
cat(identical(as.list(sum), list(NULL)), "\n", sep = "")
