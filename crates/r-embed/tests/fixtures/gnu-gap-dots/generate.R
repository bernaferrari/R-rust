# Generate with Homebrew GNU R 4.6.1 from the repository root:
# /opt/homebrew/Cellar/r/4.6.1/bin/Rscript crates/r-embed/tests/fixtures/gnu-gap-dots/generate.R

out <- "crates/r-embed/tests/fixtures/gnu-gap-dots"
dir.create(out, recursive = TRUE, showWarnings = FALSE)

# DDVAL.OP 0 / RETURN  —  function(...) ..1
saveRDS(compiler::cmpfun(function(...) ..1),
        file.path(out, "ddval1.rds"), version = 2, compress = FALSE)

# DDVAL.OP 0 / RETURN  —  function(...) ..2
saveRDS(compiler::cmpfun(function(...) ..2),
        file.path(out, "ddval2.rds"), version = 2, compress = FALSE)
