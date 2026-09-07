## Factor subsetting, assignment, and comparison semantics

## `[.factor: subsetting a factor returns a factor with the original
## levels (base ships the S3 method; the default strips attributes).
f <- factor(c("a", "b", "a"))
cat(class(f[2]), levels(f[2]), as.character(f[2]), as.integer(f[2]), "\n", sep = "|")
cat(class(f[c(TRUE, FALSE, TRUE)]), sum(as.integer(f[c(TRUE, FALSE, TRUE)])), "\n", sep = "|")

## `[<-.factor with character values matches them against the levels;
## unknown levels become NA with a warning that calling handlers can
## intercept and muffle via the dynamically scoped muffleWarning
## restart (upstream semantics).
g <- factor(c("a", "b"))
g[1] <- "b"
cat(class(g), as.character(g), as.integer(g), "\n", sep = "|")
h <- factor(c("a", "b"))
caught <- character()
withCallingHandlers(
  h[2] <- "zz",
  warning = function(w) {
    caught <<- c(caught, conditionMessage(w))
    invokeRestart("muffleWarning")
  }
)
cat(paste(caught, collapse = "|"), as.character(h), is.na(h[2]), "\n", sep = "|")

## Plain warning() interception and muffle (both handlers see their
## warnings; nothing reaches the default print path).
got <- character()
withCallingHandlers(
  { warning("w-one"); warning("w-two") },
  warning = function(w) {
    got <<- c(got, conditionMessage(w))
    invokeRestart("muffleWarning")
  }
)
cat(paste(got, collapse = "|"), "\n", sep = "")

## tryCatch exiting handlers and suppressWarnings keep working.
cat(tryCatch(warning("tc"), warning = function(w) paste("caught", conditionMessage(w))), "\n", sep = "")
cat(suppressWarnings({ warning("sw"); "sw-ok" }), "\n", sep = "")

## Unordered-factor ==/!= compares the LEVEL STRINGS of the elements
## (factor vs character matches the character against the levels).
k <- factor(c("#", "/", "x"))
cat(k[2] == "/", k[1] == "#", k[3] == "/", "\n", sep = "|")
cat(all((k == c("#", "/", "q")) == c(TRUE, TRUE, FALSE)), "\n", sep = "")
cat(sum(k != "#"), "\n", sep = "")

## Vectorized factor == against a character vector.
cat(paste(factor(c("a", "b")) == "a", collapse = "|"), "\n", sep = "")
