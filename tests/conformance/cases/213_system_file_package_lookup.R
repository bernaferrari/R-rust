root <- file.path(tempdir(), "rport_system_file")
pkg <- file.path(root, "tiny")

unlink(root, recursive = TRUE)
dir.create(file.path(pkg, "R"), recursive = TRUE)

writeLines(
  c(
    "Package: tiny",
    "Version: 0.0.1",
    "Title: Tiny",
    "Description: Tiny package for system.file conformance.",
    "License: MIT"
  ),
  file.path(pkg, "DESCRIPTION")
)

.libPaths(root)

print(basename(find.package("tiny")))
print(basename(system.file(package = "tiny")))
print(basename(system.file("R", package = "tiny")))
print(system.file("missing", package = "tiny"))
print(system.file(package = "definitely_missing"))
