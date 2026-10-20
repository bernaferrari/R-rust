# Original GNU R r90451: filenames and explicit binary connections agree.
directory <- tempfile('rport-binary-'); dir.create(directory)
input <- file.path(directory, 'input.bin'); output <- file.path(directory, 'output.bin')
writeBin(as.raw(c(0,1,127,128,255,0)),input)
stopifnot(identical(readBin(input,'raw',n=6L),as.raw(c(0,1,127,128,255,0))),
          identical(readBin(input,'integer',n=3L,size=2L,signed=FALSE,endian='little'),c(256L,32895L,255L)),
          identical(readBin(input,'raw',n=0L),raw()))
stopifnot(identical(readBin(as.raw(c(4,5,6)),'raw',n=2L),as.raw(c(4,5))),
          identical(tryCatch(readBin(character(),'raw'),error=function(e)conditionMessage(e)),"invalid 'description' argument"),
          identical(tryCatch(readBin(NA_character_,'raw'),error=function(e)conditionMessage(e)),"invalid 'description' argument"))
connection<-file(input,'rb'); saved<-readBin(connection,'raw',n=6L); close(connection)
stopifnot(identical(saved,readBin(input,'raw',n=6L)))
visible<-withVisible(writeBin(as.raw(c(0,1,255)),output))
stopifnot(is.null(visible$value),!visible$visible)
writeBin(c(1L,256L),output,size=2L,endian='big')
stopifnot(identical(readBin(output,'raw',n=4L),as.raw(c(0,1,1,0))),
          identical(readBin(output,'integer',n=2L,size=2L,endian='big'),c(1L,256L)))
operations <- list(
    list(quote(writeBin(1L,output,endian='bad')), "invalid 'endian' argument", 2),
    list(quote(writeBin(list(1L),output)), "can only write vector objects", 2),
    list(quote(writeBin(structure(1L,x=2L),output)), "can only write vector objects", 2),
    list(quote(writeBin(1L,output,size=3L)), "size 3 is unknown on this machine", 0),
    list(quote(writeBin(as.raw(1),output,size=2L)), "size changing is not supported for raw vectors", 0),
    list(quote(writeBin(1+2i,output,size=8L)), "size changing is not supported for complex vectors", 0),
    list(quote(writeBin(1L,output,useBytes=NA)), "invalid 'useBytes' argument", 0),
    list(quote((function(){old<-options(warn=2);on.exit(options(old));writeBin(1L,output,size='x',useBytes=NA)})()), "(converted from warning) NAs introduced by coercion", 0),
    list(quote((function(){old<-options(warn=2);on.exit(options(old));writeBin('a',output,size='x')})()), "(converted from warning) NAs introduced by coercion", 0))
for (operation in operations) {
    writeBin(as.raw(c(7,8)),output)
    message <- tryCatch(eval(operation[[1]]),error=function(e){gc();conditionMessage(e)})
    stopifnot(identical(message,operation[[2]]), file.info(output)$size == operation[[3]])
}
writeBin(integer(),output,size=3L)
stopifnot(identical(readBin(output,'raw',n=8L),raw()))
writeBin('a',output,size=3L)
stopifnot(identical(readBin(output,'raw',n=8L),as.raw(c(97,0))))
stopifnot(identical(tryCatch(writeBin(1L,raw(),size=3L),error=function(e)conditionMessage(e)),
                    'size 3 is unknown on this machine'))
writeBin(as.raw(c(7,8)),output); connection<-file(output,'ab')
message<-tryCatch(writeBin(1L,connection,size=3L),error=function(e)conditionMessage(e)); close(connection)
stopifnot(identical(message,'size 3 is unknown on this machine'),
          identical(readBin(output,'raw',n=8L),as.raw(c(7,8))))
connection<-file(output,'w')
stopifnot(identical(tryCatch(readBin(connection,'raw',n=1L),error=function(e)conditionMessage(e)),
                    'can only read from a binary connection'),
          identical(tryCatch(writeBin(1L,connection,size=3L),error=function(e)conditionMessage(e)),
                    'can only write to a binary connection'))
close(connection)
visible<-withVisible(writeBin(as.raw(c(1,2)),raw()))
stopifnot(visible$visible,identical(visible$value,as.raw(c(1,2))))
writeBin(c('a','b'),output); connection<-file(output,'rb')
stopifnot(identical(readBin(connection,'character',n=0L),character()),
          identical(readBin(connection,'character',n=1L),'a'),
          identical(readBin(connection,'character',n=1L),'b'),
          identical(readBin(connection,'character',n=1L),character()))
close(connection)
stopifnot(identical(readBin(con=as.raw(c(9,8,7,6)), 'raw', 2L), as.raw(c(9,8))),
          identical(writeBin(con=raw(), c(1L,256L), size=2L, endian='big'), as.raw(c(0,1,1,0))),
          identical(readBin(what='raw', con=input, 2L), as.raw(c(0,1))))
unlink(directory,recursive=TRUE)
cat('GNU_BINARY_FILENAME_CONTRACT_PASS\n')
