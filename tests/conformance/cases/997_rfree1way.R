set.seed(1)
r <- rfree1way(3)
cat(paste(as.character(r$groups), collapse = ","), "\n", sep = "")
cat(paste(round(r$y, 4), collapse = ","), "\n", sep = "")
