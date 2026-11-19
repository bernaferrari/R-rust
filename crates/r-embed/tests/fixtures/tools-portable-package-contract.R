stopifnot(identical(tools::toTitleCase("a tale of two cities"), "a Tale of Two Cities"))
stopifnot(identical(tools::file_path_sans_ext(c("src/report.R", "data.csv")), c("src/report", "data")))
stopifnot(identical(tools::file_ext(c("src/report.R", "data.csv", "README")), c("R", "csv", "")))
cat("tools namespace and analysis helpers passed\n")
