function(flesh, skeleton=attr(flesh, "skeleton"))
{
    if (is.null(skeleton)) {
	stop("The 'flesh' argument does not contain a skeleton attribute.\n",
	     "Either ensure you unlist a relistable object, or specify the skeleton separately.")
    }
    UseMethod("relist", skeleton)
}
