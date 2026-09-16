setClass("mp1", slots = c(prec = "integer", d = "integer"))
setClass("mp", contains = "list",
         validity = function(object) {
             if (all(vapply(object@.Data, class, "") == "mp1")) {
                 return(TRUE)
             }
             "Not all components are of class 'mp1'"
         })
m0 <- new("mp")
cat(isTRUE(validObject(m0)), "\n")
m1 <- new("mp", list(new("mp1"), new("mp1", prec = 1L, d = 3:5)))
cat(isTRUE(validObject(m1)), "\n")
cat(length(m1@.Data), "\n")
cat(class(m1@.Data[[1]]), "\n")
mList <- setClass("mList2", contains = "list")
ml <- mList(list(1, letters[1:3]))
cat(length(ml), "\n")
cat(isS4(ml), "\n")

