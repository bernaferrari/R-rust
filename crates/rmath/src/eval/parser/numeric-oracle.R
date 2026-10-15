# GNU source commit bac583951b728e97b9786804d3b4081f0fe18df5 (r90451).
# Independent source-literal precision controls, pinned oracle/r-oracle.json.
# Regenerate: pinned Rscript --vanilla numeric-oracle.R > numeric-oracle.tsv
inputs <- c("0.99999999999999978", "0.99999999999999977", "0.99999999999999979",
            "0.99999999999999978i", "0.99999999999999978L", "1.9999999999999998",
            "1.9999999999999998i", "1.9999999999999998L", "1e-308", "1e-320",
            "5e-324", "1e309", "0e99999999999999", "1.e0", ".125", "010",
            "0x1.2", "0x1p-1074", "0x.", "2147483647L", "2147483648L")
hex <- function(x) paste(sprintf("%02x", as.integer(x)), collapse="")
for (s in inputs) {
    value <- suppressWarnings(eval(parse(text=s)))
    bytes <- writeBin(value, raw(), size=if(is.integer(value)) 4L else if(is.complex(value)) 16L else 8L, endian="little")
    cat(hex(charToRaw(s)), typeof(value), hex(bytes), sep="|")
    cat("\n")
}
