# Generate with Homebrew GNU R 4.6.1 from the repository root:
# /opt/homebrew/Cellar/r/4.6.1/bin/Rscript crates/r-embed/tests/fixtures/gnu-gap-colon/generate.R
#
# R Under development (unstable) (2026-08-27 r90451) / 4.6.1
# Uncompressed XDR v2 so tests can inspect the instruction stream.

out <- "crates/r-embed/tests/fixtures/gnu-gap-colon"
dir.create(out, recursive = TRUE, showWarnings = FALSE)

# GNU constant-folds 1:5 to a single LDCONST integer vector.
saveRDS(compiler::cmpfun(function() 1:5),
        file.path(out, "colon-const.rds"), version = 2, compress = FALSE)

# GETVAR / LDCONST / COLON.OP / RETURN
saveRDS(compiler::cmpfun(function(x) x:3L),
        file.path(out, "colon-var.rds"), version = 2, compress = FALSE)

# BASEGUARD / GETVAR / SEQALONG.OP / RETURN
saveRDS(compiler::cmpfun(function(x) seq_along(x)),
        file.path(out, "seq-along.rds"), version = 2, compress = FALSE)

# BASEGUARD / GETVAR / SEQLEN.OP / RETURN
saveRDS(compiler::cmpfun(function(n) seq_len(n)),
        file.path(out, "seq-len.rds"), version = 2, compress = FALSE)
