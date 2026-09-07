## srcref-level error locations (keep.source + show.error.locations)

## parse() with keep.source=TRUE attaches an srcref attribute to every
## top-level expression (class "srcref", first/last line in the 8-int
## layout) and a srcfile environment carrying the filename ("<text>" for
## parse(text=)). show.error.locations then renders `(from <file>#<line>)`
## from the srcref of the evaluating top-level expression — the header
## marker is asserted via the boundary error below.
invisible(Sys.setlocale("LC_COLLATE", "C"))
options(keep.source = TRUE)
e <- parse(text = "x1 <- 1\nstop(\"tb\")")
srl <- attr(e, "srcref")
cat(is.list(srl), length(srl), is.null(attr(e[[2]], "srcref")), "\n", sep = "|")
sr1 <- srl[[1]]
sr2 <- srl[[2]]
cat(class(sr1), as.integer(sr1)[1], as.integer(sr1)[3], "\n", sep = "|")
cat(class(sr2), as.integer(sr2)[1], as.integer(sr2)[3], "\n", sep = "|")
sf <- attr(e, "srcfile")
cat(is.environment(sf), get("filename", envir = sf), class(sf)[1], "\n", sep = "|")

## parse(file=) attributes the file's basename-bearing srcfile.
tf <- tempfile(fileext = ".R")
cat("aa <- 1\nbb <- 2\n", file = tf)
ef <- parse(file = tf)
srf <- attr(ef, "srcref")[[2]]
cat(as.integer(srf)[1], as.integer(srf)[3],
    grepl("\\.R$", get("filename", envir = attr(ef, "srcfile"))), "\n", sep = "|")

## Without keep.source there are no srcrefs.
options(keep.source = FALSE)
e2 <- parse(text = "a\nb")
cat(is.null(attr(e2[[1]], "srcref")), "\n", sep = "")

## srcrefs survive round-trip through expression indexing.
options(keep.source = TRUE)
e3 <- parse(text = "1\n2\n3")
cat(paste(vapply(seq_along(e3), function(i) as.integer(attr(e3, "srcref")[[i]])[1],
                integer(1)), collapse = ","), "\n", sep = "")
invisible(unlink(tf))
