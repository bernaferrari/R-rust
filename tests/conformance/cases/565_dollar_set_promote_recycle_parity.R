## `$<-` promotion and data.frame column recycling

## Assigning into a NULL target promotes it to a list (upstream
## NULL$a <- 1 == list(a=1)). R6 relies on this chain when a class has
## no user methods: get_functions returns NULL and the following
## $clone <- f must still land on a list.
x <- NULL
x$a <- 1
cat(typeof(x), names(x), x$a, "\n", sep = "|")
y <- NULL
y$f <- function() 7
cat(is.list(y), y$f(), "\n", sep = "|")

## $<-.data.frame recycles a length-1 atomic value to the frame's row
## count (whisker/fortunes build columns this way).
df <- data.frame(u = 1:3, v = c("p", "q", "r"))
df$w <- 9
cat(length(df$w), paste(df$w, collapse = ""), "\n", sep = "|")
df$s <- factor(c(""), levels = c("", "x", "y"))
cat(length(df$s), as.integer(df$s), "\n", sep = "|")
lv <- c("", "x")
df$t <- factor("", levels = lv)
cat(length(df$t), paste(as.integer(df$t), collapse = ","), "\n", sep = "|")

## Named-list assignment into an existing column replaces 1:1 (no
## recycling when lengths already match).
df$u <- c(7, 8, 9)
cat(paste(df$u, collapse = ","), "\n", sep = "")
