# Compare numeric contracts at a declared precision; generic format() is
# tested separately. Expected warnings from empty/recycled inputs are muted.
fmt <- function(x) paste(ifelse(is.na(x), ifelse(is.nan(x), "NaN", "NA"),
    ifelse(is.infinite(x), ifelse(x < 0, "-Inf", "Inf"), sprintf("%.7g", x))), collapse=",")
suppressWarnings({

# Histogram interval ownership follows C_BinCount: right-closed by default.
h <- hist(c(0, 1, 2), breaks=c(0, 1, 2), fuzz=0,
          right=TRUE, include.lowest=TRUE, plot=FALSE)
cat("hist-right=", fmt(h$counts), " ", fmt(h$density), " ", fmt(h$mids), "\n", sep="")
h <- hist(c(0, 1, 2), breaks=c(0, 1, 2), fuzz=0,
          right=FALSE, include.lowest=TRUE, plot=FALSE)
cat("hist-left=", fmt(h$counts), "\n", sep="")
h <- hist(c(.05, .95, 1.05, 1.95, 2), breaks=c(0, 1, 2),
          fuzz=.1, right=TRUE, include.lowest=TRUE, plot=FALSE)
cat("hist-fuzzy=", fmt(h$counts), "\n", sep="")
h <- hist(c(0, .2, .8, 1.1, 2.8), breaks=c(0, 1, 3), plot=FALSE)
cat("hist-nonuniform=", fmt(h$counts), " ", fmt(h$density), " ", fmt(h$mids), "\n", sep="")
h <- hist(c(-1, 0, 1, 2), breaks=function(z) c(-1, 0, 1, 3), plot=FALSE)
cat("hist-function=", fmt(h$breaks), " ", fmt(h$counts), "\n", sep="")
h <- hist(rep(1, 4), plot=FALSE)
cat("hist-constant=", fmt(h$breaks), " ", fmt(h$counts), " ", fmt(h$density), "\n", sep="")
h <- hist(numeric(), breaks=c(0, 1), plot=FALSE)
cat("hist-empty=", fmt(h$counts), " ", fmt(h$density), "\n", sep="")

cat("bar-vector=", fmt(barplot(c(2, 4, 3), plot=FALSE)), "\n", sep="")
m <- matrix(c(2, 4, 3, 1, 5, 2), nrow=2)
cat("bar-stacked=", fmt(barplot(m, beside=FALSE, plot=FALSE)), "\n", sep="")
cat("bar-beside=", fmt(barplot(m, beside=TRUE, plot=FALSE)), "\n", sep="")
cat("bar-width-space=", fmt(barplot(m, width=c(1, 2), space=c(.2, .4),
                                    offset=c(0, 1), plot=FALSE)), "\n", sep="")

b <- boxplot(c(1, 2, 3, 4, 100), plot=FALSE)
cat("box-outlier=", fmt(as.vector(b$stats)), " ", fmt(b$n), " ",
    fmt(b$conf), " ", fmt(b$out), " ", fmt(b$group), "\n", sep="")
b <- boxplot(list(a=c(1, 2, 3), b=c(10, 20, 100)), plot=FALSE)
cat("box-groups=", fmt(as.vector(b$stats)), " ", fmt(b$n), " ",
    paste(b$names, collapse=","), "\n", sep="")
b <- boxplot(c(NA, Inf, -Inf), plot=FALSE)
cat("box-infinite=", fmt(as.vector(b$stats)), " ", fmt(b$n), " ",
    fmt(b$out), "\n", sep="")
b <- boxplot(1:4, plot=FALSE)
cat("box-hinges=", fmt(as.vector(b$stats)), " ", fmt(b$n), "\n", sep="")
b <- boxplot(c(1, 2, 3, 4, 100), range=0, plot=FALSE)
cat("box-range0=", fmt(as.vector(b$stats)), " ", fmt(b$out), "\n", sep="")
b <- boxplot(c(1, 2), c(3, 4), plot=FALSE)
cat("box-extra-groups=", fmt(as.vector(b$stats)), " ", fmt(b$n), "\n", sep="")
cat("pretty-default=", fmt(pretty(c(1, 4))), "\n", sep="")
cat("pretty-bounds-false=", fmt(pretty(c(1, 4), bounds=FALSE)), "\n", sep="")
cat("pretty-empty=", fmt(pretty(numeric())), " ", fmt(pretty(NULL)), "\n", sep="")

})
