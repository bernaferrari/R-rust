# GNU R base closure; pinned source bac583951b728e97b9786804d3b4081f0fe18df5.
function (x, rowlab = dn[[1]], collab = dn[[2]], quote = TRUE,
          right = FALSE, na.print = NULL, ...)
{
    x <- as.matrix(x)
    dn <- dimnames(x)
    .Internal(prmatrix(x, rowlab, collab, quote, right, na.print))
}
