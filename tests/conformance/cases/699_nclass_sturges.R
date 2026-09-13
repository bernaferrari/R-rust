cat(nclass.Sturges(1:10), "\n", sep = "")
cat(nclass.Sturges(1:100), "\n", sep = "")
cat(isTRUE(nclass.Sturges(numeric(0)) < 0 && is.infinite(nclass.Sturges(numeric(0)))), "\n", sep = "")
cat(nclass.Sturges(1), "\n", sep = "")
