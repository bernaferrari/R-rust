# GNU source commit bac583951b728e97b9786804d3b4081f0fe18df5 (r90451).
# Regenerate: pinned Rscript --vanilla mget-fallback-oracle.R > mget-fallback-oracle.out
e <- new.env(); count <- 0L
f <- function(name) {count <<- count + 1L; name}
cat(tryCatch({mget(c("missing", ""), e, ifnotfound=f); "accepted"},error=function(e)conditionMessage(e)),"|", count, "\n",sep="")
cat(tryCatch({mget(c("missing", ""), e, ifnotfound=list(f)); "accepted"},error=function(e)conditionMessage(e)),"|", count, "\n",sep="")
for (f in list(abs, quote)) cat(tryCatch({mget("missing",e,ifnotfound=f);"accepted"},error=function(e)conditionMessage(e)),"\n",sep="")
cat(paste(unlist(mget(c("a","b"), e, ifnotfound=c(17L,19L))),collapse=","),"\n",sep="")
