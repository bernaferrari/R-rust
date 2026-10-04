# Independent GNU R r90451 controls. Run with the pinned oracle Rscript.
# Traces observe the genuine graphics namespace functions and actual null-PDF drawing.
pdf(NULL)
.axis_records <- list(); .rect_colors <- character()
trace("axis", where=asNamespace("graphics"), print=FALSE, tracer=quote({
 if (!is.null(at)) .GlobalEnv$.axis_records <- c(.GlobalEnv$.axis_records, list(list(side=side,at=as.vector(at),labels=labels)))
}))
trace("rect", where=asNamespace("graphics"), print=FALSE, tracer=quote({
 .GlobalEnv$.rect_colors <- c(.GlobalEnv$.rect_colors,as.character(col))
}))
cases <- list(
 named_vector=list(height=setNames(c(2,4,3),c("A","B","C")),col=c("red","blue","green")),
 one_dimensional_table=list(height=table(factor(c("a","a","b","c","c","c"))),col=c("red","blue","green")),
 vector_one_label=list(height=c(2,4,3),names.arg="group"),
 grouped_matrix=list(height=matrix(1:6,nrow=2,dimnames=list(NULL,c("A","B","C"))),beside=TRUE,col=c("red","blue")),
 grouped_full_labels=list(height=matrix(1:6,nrow=2),beside=TRUE,names.arg=LETTERS[1:6],col=c("red","blue")),
 stacked_matrix=list(height=matrix(1:6,nrow=2,dimnames=list(NULL,c("A","B","C"))),col=c("red","blue"))
)
for (name in names(cases)) {
 .axis_records <- list();.rect_colors <- character()
 result <- do.call(graphics::barplot,c(cases[[name]],list(axes=FALSE,ann=FALSE)))
 record <- .axis_records[[1L]]
 stopifnot(length(record$at)==length(record$labels))
 if (name=="vector_one_label") stopifnot(length(record$at)==1L,identical(record$at,as.vector(colMeans(result))))
 if (name %in% c("named_vector","one_dimensional_table")) stopifnot(identical(.rect_colors,c("red","blue","green")))
 cat(name,"\n");dput(list(dim=dim(result),midpoints=as.vector(result),at=record$at,labels=record$labels,colors=.rect_colors))
}
msg<-tryCatch(barplot(c(2,4,3),names.arg=c("A","B")),error=conditionMessage)
stopifnot(identical(msg,"incorrect number of names"))
cat("wrong_names: ",msg,"\n",sep="")
untrace("axis",where=asNamespace("graphics"));untrace("rect",where=asNamespace("graphics"));dev.off()
pdf(NULL)
for (case in list(
 list(height=c(2,4,3),col="grey"),
 list(height=c(2,4,3),col=c("red","blue")),
 list(height=matrix(1:6,nrow=2),beside=TRUE,col=c("red","blue","green")),
 list(height=matrix(1:6,nrow=2),col=c("red","blue","green")),
 list(height=c(2,4,3),col=NA_character_))) {
 result <- do.call(graphics::barplot,c(case,list(border=NA,axes=FALSE,axisnames=FALSE,ann=FALSE)))
 stopifnot(is.numeric(result))
}
stopifnot(identical(unname(col2rgb(NA_character_,alpha=TRUE)[,1L]),c(255L,255L,255L,0L)))
dev.off()
cat("barplot canonical axis/vector/color neighbors: PASS\n")
