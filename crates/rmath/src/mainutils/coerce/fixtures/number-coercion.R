# Independent controls for GNU R scalar numeric text conversion.
# Regenerate: pinned Rscript --vanilla number-coercion.R > number-coercion.tsv
inputs <- c("", " ", "\t\n", "0", "-0", "+0", "1", "-2.5", ".5", "1.",
            "  +1.25e2 ", "3.141592653589793", "1e-308", "5e-324", "1e309",
            "1e999999999999", "0e999999999999", "-0e999999999999", "1e-999999999999",
            "NaN", "nan", "NAN", "-NaN", "+NaN", "Inf", "infinity", "-INF", "+Infinity",
            "NA", "nan(payload)", "NaNx", "Infinityx", "1e", "1e+", "1e-", "1junk", ".", "+", "-",
            "0x1", "0Xf", "0x1p3", "0x1.8", "0x.8", "0x1.8p-1", "0x1p-1074",
            "0x1p1024", "0x0p99999999", "0xg", "0x.", "0x1p", "0x1p+", "0x1.2.3",
            "2147483647", "2147483648", "-2147483648", "3.9", "1\v", "1\f",
            "3i", "1+2i", "1-2i", "1e-2+3e+2i", "1+0x1p-3i", "i", "+i", "1+i")
hex <- function(x) paste(sprintf("%02x", as.integer(x)), collapse="")
encode <- function(x, mode) {
  if(mode == "integer") return(hex(writeBin(x, raw(), size=4L, endian="little")))
  hex(writeBin(x, raw(), size=if(mode == "complex") 16L else 8L, endian="little"))
}
for(s in inputs) for(mode in c("double", "integer", "complex")) {
  warnings <- character()
  answer <- withCallingHandlers(switch(mode, double=as.double(s), integer=as.integer(s), complex=as.complex(s)),
       warning=function(w) {warnings <<- c(warnings, conditionMessage(w)); invokeRestart("muffleWarning")})
  cat(hex(charToRaw(s)), mode, encode(answer,mode), paste(warnings,collapse=";"), sep="|")
  cat("\n")
}
