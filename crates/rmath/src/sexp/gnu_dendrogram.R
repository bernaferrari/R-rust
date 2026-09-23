{
is.leaf <- function(object) (is.logical(L <- attr(object, "leaf"))) && L

.memberDend <- function(x) {
    attr(x,"x.member") %||% ( attr(x,"members") %||% 1L )
}

.midDend <- function(x) attr(x, "midpoint") %||% 0

.validity.hclust <- function(x, merge = x$merge, order = TRUE) {
    if (!is.matrix(merge) || ncol(merge) != 2)
	return("invalid dendrogram")
    if (any(as.integer(merge) != merge))
	return("'merge' component in dendrogram must be integer")
    n1 <- nrow(merge)
    n <- n1+1L
    if(length(x$height) != n1) return("'height' is of wrong length")
    if(order && length(x$order ) != n ) return("'order' is of wrong length")
    if(identical(sort(as.integer(merge)), c(-(n:1L), +seq_len(n-2L))))
	TRUE
    else
	"'merge' matrix has invalid contents"
}

as.dendrogram <- function(object, ...) UseMethod("as.dendrogram")

as.dendrogram.dendrogram <- function(object, ...) object

as.dendrogram.hclust <- function (object, hang = -1, check = TRUE, ...)
{
    nolabels <- is.null(object$labels)
    merge <- object$merge
    if(check && !isTRUE(msg <- .validity.hclust(object, merge, order=nolabels)))
	stop(msg)
    if(nolabels)
	object$labels <- seq_along(object$order)
    .rport_as_dendrogram(object, hang)
}

`[[.dendrogram` <- function(x, ..., drop = TRUE) {
    if(!is.null(r <- NextMethod("[[")))
        structure(r, class = "dendrogram")
}

nobs.dendrogram <- function(object, ...) attr(object, "members")

midcache.dendrogram <- function (x, type = "hclust", quiet=FALSE)
{
    type <- match.arg(type)
    stopifnot( inherits(x, "dendrogram") )
    setmid <- function(d, type) {
	depth <- 0L
	kk <- integer()
	jj <- integer()
	dd <- list()
	repeat {
	    if(!is.leaf(d)) {
		k <- length(d)
		if(k < 1)
		    stop("dendrogram node with non-positive #{branches}")
		depth <- depth + 1L
		kk[depth] <- k
		if(storage.mode(jj) != storage.mode(kk))
		    storage.mode(jj) <- storage.mode(kk)
		dd[[depth]] <- d
		d <- d[[jj[depth] <- 1L]]
		next
	    }
	    while(depth) {
		k <- kk[depth]
		j <- jj[depth]
		r <- dd[[depth]]
		r[[j]] <- unclass(d)
		if(j < k) break
		depth <- depth - 1L
		midS <- sum(vapply(r, .midDend, 0))
		if(!quiet && type == "hclust" && k != 2)
		    warning("midcache() of non-binary dendrograms only partly implemented")
		attr(r, "midpoint") <- (.memberDend(r[[1L]]) + midS) / 2
		d <- r
	    }
	    if(!depth) break
	    dd[[depth]] <- r
	    d <- r[[jj[depth] <- j + 1L]]
	}
	d
    }
    setmid(x, type=type)
}

reorder.dendrogram <- function(x, wts, agglo.FUN = sum, ...)
{
    if( !inherits(x, "dendrogram") )
	stop("'reorder.dendrogram' requires a dendrogram")
    .rport_reorder_dendrogram(x, wts)
}


nleaves <- function (node) {
    if (is.leaf(node))
	return(1L)
    todo <- NULL
    count <- 0L
    repeat {
	while (length(node)) {
	    child <- node[[1L]]
	    node <- node[-1L]
	    if (is.leaf(child)) {
		count <- count + 1L
	    } else {
		todo <- list(node=child, todo=todo)
	    }
	}
	if (is.null(todo)) {
	    break
	} else {
	    node <- todo$node
	    todo <- todo$todo
	}
    }
    count
}

as.hclust.dendrogram <- function(x, ...)
{
    stopifnot(is.list(x), length(x) == 2L)
    n <- nleaves(x)
    stopifnot(n == attr(x, "members"))
    ord <- integer(n)
    labsu <- character(n)
    n.h <- n - 1L
    height <- numeric(n.h)
    myIdx <- matrix(NA_integer_, 2L, n.h)
    merge <- matrix(NA_integer_, 2L, n.h)
    position <- 0L
    stack <- NULL
    leafCount <- 0L
    nodeCount <- 0L
    repeat {
	while (length(x)) {
	    if (position == 0L) {
		nodeCount <- nodeCount + 1L
		myNodeIndex <- nodeCount
		if (nodeCount != 1L)
		    myIdx[, nodeCount] <- c(stack$position, stack$myNodeIndex)
		height[nodeCount] <- attr(x, "height")
	    }
	    position <- position + 1L
	    child <- x[[1L]]
	    x <- x[-1L]
	    if (is.leaf(child)) {
		leafCount <- leafCount + 1L
		labsu[leafCount] <- attr(child, "label")
		ord[leafCount] <- as.integer(child)
		merge[position, myNodeIndex] <- -ord[leafCount]
	    } else {
		stopifnot(length(child) == 2L)
		stack <- list(node = x, position = position,
			      myNodeIndex = myNodeIndex, stack = stack)
		x <- child
		position <- 0L
	    }
	}
	if (is.null(stack)) break
	position <- stack$position
	x <- stack$node
	myNodeIndex <- stack$myNodeIndex
	stack <- stack$stack
    }
    iOrd <- sort.list(ord)
    if (!identical(ord[iOrd], seq_len(n)))
	stop(gettextf("dendrogram entries must be 1,2,..,%d (in any order), to be coercible to \"hclust\"", n), domain = NA)
    ii <- sort.list(height, decreasing = TRUE)[n.h:1L]
    stopifnot(ii[n.h] == 1L)
    k <- seq_len(n.h - 1L)
    merge[t(myIdx[, ii[k]])] <- +k
    structure(list(merge = t(merge[, ii]),
		   height = height[ii],
		   order = ord,
		   labels = labsu[iOrd],
		   call = match.call(),
		   method = NA_character_,
		   dist.method = NA_character_),
	      class = "hclust")
}

as.dendrogram
}
