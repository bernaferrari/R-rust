# Independent scalar stream tokens from the authenticated r90451 oracle.
args <- commandArgs(TRUE)
stopifnot(length(args) == 1L, R.version[["svn rev"]] == "90451")
bits <- function(x) paste(sprintf("%02x", as.integer(writeBin(x, raw(), size=8L,
                                                         endian="big"))), collapse="")
token <- function(x, mode) {
  lines <- strsplit(rawToChar(serialize(x, NULL, ascii=mode, version=2L)), "\n")[[1L]]
  stopifnot(length(lines) == 7L, lines[[1L]] == "A", lines[[6L]] == "1")
  lines[[7L]]
}
values <- c(zero=0, negative_zero=-0, one=1, fraction=0.1, pi=pi,
            na=NA_real_, nan=NaN, positive_inf=Inf, negative_inf=-Inf,
            smallest_subnormal=.Machine$double.xmin*.Machine$double.eps,
            smallest_normal=.Machine$double.xmin, maximum=.Machine$double.xmax,
            small_threshold=1e-4, small_scientific=1e-5, large_fixed=1e15,
            large_scientific=1e16, rounding_up=9.999999999999999e-5,
            rounding_down=0.9999999999999999,
            tiny_negative=-.Machine$double.xmin*.Machine$double.eps)
set.seed(90451L)
values <- c(values, setNames(runif(24L, -1, 1)*10^seq(-300, 300, length.out=24L),
                            paste0("finite_", seq_len(24L))))
rows <- lapply(seq_along(values), function(i) {
  x <- unname(values[[i]])
  c(case=names(values)[[i]], bits=bits(x), decimal=token(x, TRUE), hex=token(x, NA))
})
write.table(do.call(rbind, rows), file=args[[1L]], sep="\t", quote=FALSE,
            row.names=FALSE, col.names=TRUE)
cat(length(rows), "independent GNU numeric stream cases\n")
