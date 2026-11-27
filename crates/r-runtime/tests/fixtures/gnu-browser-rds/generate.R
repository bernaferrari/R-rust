# Regenerate with the independently installed pinned oracle, not the port:
# pinned Rscript --vanilla generate.R OUTPUT_DIRECTORY
arguments <- commandArgs(trailingOnly=TRUE)
directory <- if(length(arguments)) arguments[[1L]] else "."
dir.create(directory,recursive=TRUE,showWarnings=FALSE)
value <- list(x=c(1L,2L,3L),message="café",missing=NA_real_)
saveRDS(value,file.path(directory,"plain.rds"),compress=FALSE)
saveRDS(value,file.path(directory,"gzip.rds"))
stopifnot(identical(readRDS(file.path(directory,"plain.rds")),value),
          identical(readRDS(file.path(directory,"gzip.rds")),value))
cat("GNU_BROWSER_RDS_FIXTURES_PASS\n")
