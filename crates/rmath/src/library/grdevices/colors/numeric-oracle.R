# GNU source commit bac583951b728e97b9786804d3b4081f0fe18df5 (r90451).
# Independent numeric-color controls, pinned oracle/r-oracle.json GNU R.
# Regenerate: pinned Rscript --vanilla numeric-oracle.R > numeric-oracle.tsv
# All accepted values are finite and truncate inside the C int range. Never
# probe GNU's undefined overflow-to-int conversion as an oracle.
inputs <- c("00", "0.0", "0e3", "0x0", "0x0p99999999999999999999",
            "0x.", "0x", "0x1.2.3", "0x1", "0Xf", "0x1.", "0x.1", "0x1p0",
            "0x1.8p1", "0x1p", "0x1p+", "0x1p-", "0x1p0tail", "0x1.2.3p0",
            "1", "1.", "1.e0", "1e", "1e+", "1e-", "1 ", "1\t", "1,0", "1_0",
            "1.9999999999999998", "1.9999999999999999", "0.99999999999999999",
            "0x1.fffffffffffff7", "0x1.fffffffffffff8",
            "0x1.fffffffffffff80000000000000001", "0x1.00000000000008000000000000001",
            "2147483647", "2147483647.9999997", "0x7fffffff.fffffc",
            "0x1p-1074", "0x1p-1075", "1e-9999999999999999999")
hex <- function(x) paste(sprintf("%02x", as.integer(charToRaw(x))), collapse="")
for (s in inputs) {
    result <- tryCatch(paste(col2rgb(s, alpha=TRUE), collapse=","),
                       error=function(e) "error")
    cat(hex(s), result, sep="|")
    cat("\n")
}
