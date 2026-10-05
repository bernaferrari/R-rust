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
unlink(directory,recursive=TRUE)
cat('GNU_BINARY_FILENAME_CONTRACT_PASS\n')
