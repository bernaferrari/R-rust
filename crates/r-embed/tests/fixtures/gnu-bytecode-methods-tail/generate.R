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
`stamp<-` <- function(label, object, value) {attr(object, label) <- value; object}
stamp(object=x, label="mark") <- 7L
stopifnot(identical(attr(x, "mark"), 7L))
x <- 1L
`stamp<-` <- function(x,value) {attr(x,"code") <- substitute(x); attr(x,"rhs") <- substitute(value); x}
stamp(x) <- quote(y)
stopifnot(identical(attr(x,"code"), as.name("*tmp*")), identical(attr(x,"rhs"), quote(quote(y))))
