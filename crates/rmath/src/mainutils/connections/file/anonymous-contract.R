# Identical valid branches run against pinned GNU r90451 and the public port.
connection <- file("")
stopifnot(isOpen(connection), isOpen(connection,"r"), isOpen(connection,"w"),
          isSeekable(connection), identical(summary(connection)$description,""),
          identical(summary(connection)$mode,"w+"))
stopifnot(identical(tryCatch(readBin(connection,"raw",n=1L),error=function(e)conditionMessage(e)),
                    "can only read from a binary connection"),
          identical(tryCatch(writeBin(1L,connection,size=3L),error=function(e)conditionMessage(e)),
                    "can only write to a binary connection"))
writeLines(c("alpha","beta"),connection)
gc()
stopifnot(identical(readLines(connection,warn=FALSE),c("alpha","beta")),
          identical(seek(connection,0,rw="read"),11))
writeLines("gamma",connection)
stopifnot(identical(readLines(connection,warn=FALSE),c("alpha","beta","gamma")))
close(connection)

connection <- file("")
writeLines(c(NA_character_,"after"),connection)
stopifnot(identical(readLines(connection,warn=FALSE),c("NA","after")))
close(connection)

connection <- file("", "w+b")
writeBin(c("a","b"),connection)
stopifnot(identical(readBin(connection,"character",n=0L),character()),
          identical(readBin(connection,"character",n=1L),"a"),
          identical(readBin(connection,"character",n=1L),"b"),
          identical(readBin(connection,"character",n=1L),character()))
close(connection)

connection <- file("", "w+b")
stopifnot(identical(summary(connection)$mode,"w+b"),
          identical(summary(connection)$text,"binary"))
writeBin(as.raw(c(1,2,3)),connection)
gc()
stopifnot(identical(readBin(connection,"raw",n=8L),as.raw(c(1,2,3))),
          identical(seek(connection,0,rw="write"),3))
writeBin(as.raw(9),connection)
stopifnot(identical(seek(connection,0,rw="read"),3),
          identical(readBin(connection,"raw",n=8L),as.raw(c(9,2,3))),
          identical(seek(connection,5,rw="write"),1))
writeBin(as.raw(8),connection)
stopifnot(identical(seek(connection,-3,origin="end",rw="read"),3),
          identical(readBin(connection,"raw",n=8L),as.raw(c(0,0,8))))
seek(connection,0,rw="read")
stopifnot(identical(readBin(connection,"raw",n=8L),as.raw(c(9,2,3,0,0,8))))
close(connection)

# A query changes the mode but leaves the physical offset in place. This
# observable GNU behavior requires all four cursor state fields.
connection <- file("", "w+b")
writeBin(as.raw(c(1,2,3)),connection)
stopifnot(identical(seek(connection,NA,rw="read"),0),
          identical(seek(connection,NA,rw="write"),3),
          identical(readBin(connection,"raw",n=8L),raw()),
          identical(seek(connection,NA,rw="read"),3))
close(connection)

connection <- suppressWarnings(file("", "rb"))
stopifnot(identical(summary(connection)$mode,"w+"),isOpen(connection,"r"),isOpen(connection,"w"))
close(connection)

# Exact formal names precede partial names and remaining positional arguments.
connection <- file("", "w+b")
writeBin(as.raw(c(1,2,3,4)),connection)
seek(connection,0,orig="end",r="read")
stopifnot(identical(seek(connection,where=NA_real_,rw="read"),4))
seek(rw="read",con=connection,0,origin="end")
stopifnot(identical(seek(connection,where=NA_real_,rw="read"),4))
seek(connection,0,orig="end",origin="start",rw="read")
stopifnot(identical(seek(connection,where=NA_real_,rw="read"),0))
seek(connection,0,wat="end",rw="read")
stopifnot(identical(seek(connection,where=NA_real_,rw="read"),0),
          identical(tryCatch(seek(connection,origin="start",origin="end"),
                             error=function(e)conditionMessage(e)),
                    'formal argument "origin" matched by multiple actual arguments'),
          identical(tryCatch(seek(connection,orig="start",o="end"),
                             error=function(e)conditionMessage(e)),
                    'formal argument "origin" matched by multiple actual arguments'))
close(connection)
TRUE
