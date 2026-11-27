# Generated with oracle/r-oracle.json's GNU R commit
# bac583951b728e97b9786804d3b4081f0fe18df5 (R 4.7.0 development, revision 90451).
# Uncompressed XDR version 2 permits bytecode mutation independent of source.
# This is the vector/replacement tail of methods::matchSignature, with its
# already-computed inputs made explicit to avoid unrelated namespace startup.
out <- "crates/r-embed/tests/fixtures/gnu-bytecode-methods-tail"
dir.create(out, recursive=TRUE, showWarnings=FALSE)
functions <- list(
 tail=function(anames, which, sigClasses, pkgs) {
   n <- length(anames)
   value <- rep("ANY", n)
   valueP <- rep("methods", n)
   names(value) <- anames
   value[which] <- sigClasses
   valueP[which] <- pkgs
   unspec <- value == "ANY"
   while (n > 1 && unspec[[n]]) n <- n - 1
   length(value) <- length(valueP) <- n
   attr(value, "package") <- valueP
   value
 },
 padding=function(sig, more) c(as.character(sig), rep("ANY", more)),
 oldclass=function(cl, S3Class) c(cl, S3Class),
 attribute=function(x, value) { attr(x,"package") <- value; x }
)
for (name in names(functions)) {
 saveRDS(compiler::cmpfun(functions[[name]]),file.path(out,paste0(name,".rds")),version=2,compress=FALSE)
}
x <- structure(c(x="foo"),package="fixturePackage")
stopifnot(identical(functions$tail(c("x","y","z","w"),1L,"foo","fixturePackage"),x))
stopifnot(identical(functions$padding("foo",3L),c("foo","ANY","ANY","ANY")))
stopifnot(identical(functions$oldclass("child",c("parent","oldClass")),c("child","parent","oldClass")))
stopifnot(identical(functions$attribute(c(x="foo"),"fixturePackage"),x))

# Independent lazy/tagged setter and substitution oracle for private lowering.
x <- 1L
`stamp<-` <- function(x, label, value) {attr(x, "expression") <- substitute(label); x}
stamp(x, unbound_label) <- 7L
stopifnot(identical(attr(x, "expression"), quote(unbound_label)))
x <- 1L
# GNU discards the first object's tag; the untagged extra discriminates.
`stamp<-` <- function(label, object, value) {list(label=label,object=object,value=value)}
stamp(object=x, "mark") <- 7L
stopifnot(identical(x, list(label=1L,object="mark",value=7L)))
x <- 1L
`stamp<-` <- function(x,value) {attr(x,"code") <- substitute(x); attr(x,"rhs") <- substitute(value); x}
stamp(x) <- quote(y)
stopifnot(identical(attr(x,"code"), as.name("*tmp*")), identical(attr(x,"rhs"), quote(quote(y))))

# Observable call syntax and the assignment's retained original RHS.
sys.call.oracle <- sys.call
x <- 1L
`stamp<-` <- function(x,label,value) {attr(x,"observed") <- sys.call.oracle(); x}
stamp(x,unbound_label) <- {1L;7L}
observed <- attr(x,"observed")
stopifnot(identical(observed[[1L]],quote(`stamp<-`)), identical(observed[[2L]],quote(`*tmp*`)),
          identical(observed[[3L]],quote(unbound_label)), identical(typeof(observed[[4L]]),"promise"))
x <- 1L
rhs <- 7L
`stamp<-` <- function(x,value) {attr(value,"modified") <- TRUE; value}
result <- withVisible(stamp(x) <- rhs)
stopifnot(identical(result$value,rhs), is.null(attributes(rhs)), !result$visible, identical(attr(x,"modified"),TRUE))
x <- 1L
rhs <- c(7L,8L)
`stamp<-` <- function(x,value) {value[1L] <- 9L; value}
result <- withVisible(stamp(x) <- rhs)
stopifnot(identical(result$value,rhs), identical(rhs,c(7L,8L)), !result$visible, identical(x,c(9L,8L)))
