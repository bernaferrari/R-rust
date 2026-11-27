# Generated with the GNU R oracle pinned in oracle/r-oracle.json.
# Uncompressed XDR permits tests to change an opcode without changing source.
operators <- c(add="+", subtract="-", multiply="*", divide="/",
               equal="==", unequal="!=", less="<", less_equal="<=",
               greater_equal=">=", greater=">")
for (name in names(operators)) {
    f <- eval(call("function", as.pairlist(alist(x=, y=)),
                   call(operators[[name]], quote(x), quote(y))))
    f <- compiler::cmpfun(f)
    saveRDS(f, file.path("crates/r-embed/tests/fixtures/gnu-bytecode-arithmetic",
                         paste0(name, ".rds")), version=2, compress=FALSE)
}
