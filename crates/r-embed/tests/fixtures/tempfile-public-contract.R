set.seed(37)
seed <- .Random.seed
p <- tempfile(pattern=c("palette", "other"), fileext=c(".pdf", ".tmp"))
print(is.character(p) && length(p) == 2L && !anyDuplicated(p) && !any(file.exists(p)))
print(identical(.Random.seed, seed))
print(startsWith(basename(p[1]), "palette") && endsWith(p[1], ".pdf"))
print(startsWith(basename(p[2]), "other") && endsWith(p[2], ".tmp"))
print(!identical(p, tempfile(pattern=c("palette", "other"), fileext=c(".pdf", ".tmp"))))
