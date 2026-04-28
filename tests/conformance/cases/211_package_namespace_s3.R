root <- file.path(tempdir(), "rport_package_namespace_s3")
pkg <- file.path(root, "tiny")

unlink(root, recursive = TRUE)
dir.create(file.path(pkg, "R"), recursive = TRUE)

writeLines(
  c(
    "Package: tiny",
    "Version: 0.0.1",
    "Title: Tiny",
    "Description: Tiny package for strict package conformance.",
    "License: MIT"
  ),
  file.path(pkg, "DESCRIPTION")
)
writeLines(
  c(
    "export(tiny_generic, make_tiny, tiny_value)",
    "S3method(tiny_generic, tinything)"
  ),
  file.path(pkg, "NAMESPACE")
)
writeLines(
  c(
    "tiny_value <- function() 42L",
    "make_tiny <- function(x) { structure(list(value = x), class = \"tinything\") }",
    "tiny_generic <- function(x) UseMethod(\"tiny_generic\")",
    "tiny_generic.tinything <- function(x) paste(\"tiny\", x$value)"
  ),
  file.path(pkg, "R", "tiny.R")
)

.libPaths(root)

print(file.exists(find.package("tiny")))
print(basename(find.package("tiny")))
print(basename(.libPaths()[1]))
