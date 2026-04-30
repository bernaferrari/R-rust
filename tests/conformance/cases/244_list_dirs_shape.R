root <- file.path(tempdir(), "rport-list-dirs")
unlink(root, recursive = TRUE)
dir.create(file.path(root, "a", "b"), recursive = TRUE)
dir.create(file.path(root, "c"), recursive = TRUE)

print(paste(list.dirs(root, full.names = FALSE, recursive = FALSE), collapse = ","))
print(paste(list.dirs(root, full.names = FALSE, recursive = TRUE), collapse = ","))
full <- list.dirs(root, full.names = TRUE, recursive = FALSE)
print(paste(basename(full), collapse = ","))
