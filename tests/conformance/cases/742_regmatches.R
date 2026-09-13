cat(paste(regmatches("a12b3", gregexpr("[0-9]+", "a12b3"))[[1]], collapse = ","), "\n", sep = "")
cat(regmatches("a12b3", regexpr("[0-9]+", "a12b3")), "\n", sep = "")
