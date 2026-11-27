# Pinned GNU R bac583951b728e97b9786804d3b4081f0fe18df5,
# R 4.7.0 development revision 90451. Uncompressed XDR serialization v2.
# These base-only functions exercise real GNU subassignment instructions.
out <- "crates/r-embed/tests/fixtures/gnu-bytecode-subassignment"
dir.create(out, recursive=TRUE, showWarnings=FALSE)
functions <- list(
 single=function(x, rhs, i=1L) x[i] <- rhs,
 double=function(x, rhs, i=1L) x[[i]] <- rhs,
 whole=function(x, rhs) x[] <- rhs,
 matrix=function(x, rhs, i=1L, j=2L) x[i,j] <- rhs,
 order=function() {
   x <- c(1L,2L)
   events <- character()
   rhs <- function() {events <<- c(events,"rhs"); x <<- c(30L,40L); 7L}
   index <- function() {events <<- c(events,"index"); 1L}
   value <- withVisible(x[index()] <- rhs())
   list(result=value,x=x,events=events)
 }
)
for (name in names(functions)) environment(functions[[name]]) <- baseenv()
expected.order <- list(result=list(value=7L,visible=FALSE),x=c(7L,40L),events=c("rhs","index"))
for (compile in c(FALSE,TRUE)) {
 current <- if (compile) lapply(functions,compiler::cmpfun) else functions
 stopifnot(identical(withVisible(current$single(c(1L,2L),7L)),list(value=7L,visible=FALSE)))
 stopifnot(identical(withVisible(current$single(c(1,2),7L)),list(value=7L,visible=FALSE)))
 stopifnot(identical(withVisible(current$single(c(1,2),NA)),list(value=NA,visible=FALSE)))
 stopifnot(identical(withVisible(current$single(c(1L,2L),7.5)),list(value=7.5,visible=FALSE)))
 replacement <- structure(list(7L),note="original")
 stopifnot(identical(withVisible(current$single(list(1L,2L),replacement)),list(value=replacement,visible=FALSE)))
 stopifnot(identical(withVisible(current$double(list(1L,2L),quote(unbound_data))),list(value=quote(unbound_data),visible=FALSE)))
 stopifnot(identical(withVisible(current$double(list(1L,2L),quote(a+b))),list(value=quote(a+b),visible=FALSE)))
 stopifnot(identical(withVisible(current$whole(c(1L,2L),7L)),list(value=7L,visible=FALSE)))
 stopifnot(identical(withVisible(current$matrix(matrix(1:4,2L),7L)),list(value=7L,visible=FALSE)))
 stopifnot(identical(current$order(),expected.order))
}
for (name in names(functions)) {
 saveRDS(compiler::cmpfun(functions[[name]]),file.path(out,paste0(name,".rds")),version=2,compress=FALSE)
}
cat("GNU source/compiled subassignment value, visibility and order assertions passed\n")
