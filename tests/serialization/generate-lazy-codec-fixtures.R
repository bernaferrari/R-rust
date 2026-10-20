value <- rep.int(7L, 128L)
dir <- "crates/rmath/src/mainutils/serialize/fixtures/gnu-lazy-codecs"
writeBin(serialize(value, NULL, version=3), file.path(dir,"expected.rds"))
for (mode in 1:3) {
 path <- file.path(dir,paste0("mode",mode,".bin"))
 if(file.exists(path)) unlink(path)
 key <- .Internal(lazyLoadDBinsertValue(value,path,FALSE,mode,NULL))
 restored <- lazyLoadDBfetch(key,path,mode,NULL)
 stopifnot(identical(restored,value))
}
cat("GNU_LAZY_CODEC_FIXTURES_COMPLETE modes=3\n")
