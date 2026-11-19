anyNA.probe <- function(x, recursive=FALSE) {
  cat("method", attr(x,"id"), recursive, "\n", sep="|")
  invisible(gc())
  attr(x,"answer")
}
is.na.probe <- function(x) {
  cat("fallback", attr(x,"id"), "\n", sep="|")
  attr(x,"missing")
}
mk <- function(id,answer,missing=FALSE) structure(1,class="probe",id=id,answer=answer,missing=missing)
cases <- list(true=list(mk("t",TRUE)), false=list(mk("f",FALSE)), fallback=list(mk("b",FALSE,TRUE)), na=list(mk("n",NA)), pair=pairlist(mk("p",TRUE)), deep=list(list(mk("d",TRUE))), short=list(mk("s",TRUE),mk("late",TRUE)))
for (n in names(cases)) {
  cat("case", n, "\n", sep="|")
  result <- tryCatch(as.character(anyNA(cases[[n]], recursive=TRUE)), error=function(e) paste0("ERROR:",conditionMessage(e)))
  cat("result",result,"\n",sep="|")
}
anyNA.fail <- function(x, recursive=FALSE) { invisible(gc()); stop("nested method error") }
cat("case|error|\n")
result <- tryCatch(as.character(anyNA(list(structure(1,class="fail")),recursive=TRUE)), error=function(e) paste0("ERROR:",conditionMessage(e)))
cat("result", result, "\n", sep="|")
anyNA.reenter <- function(x, recursive=FALSE) { invisible(gc()); anyNA(list(NA_real_), recursive=TRUE) }
cat("case|reentry|\n")
cat("result", anyNA(list(structure(1,class="reenter")),recursive=TRUE), "\n", sep="|")
cat("recovery", anyNA(list(1),recursive=TRUE), "\n", sep="|")
cat("case|named|\n")
cat("result", anyNA(recursive=TRUE, x=list(mk("named", TRUE))), "\n", sep="|")
