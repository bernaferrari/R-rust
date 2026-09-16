callme <- function(a = 1, mm = c("Abc", "Bde")) {
    match.arg(mm)
}
cat(callme(), "\n", sep = "")
cat(callme(mm = "B"), "\n", sep = "")
mycaller <- function(x = 1, callme = pi) {
    callme(x)
}
cat(mycaller(), "\n", sep = "")
cat(match.arg("B", c("Abc", "Bde")), "\n", sep = "")
