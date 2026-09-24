function(pattern, x, ignore.case = FALSE, perl = FALSE,
         value = FALSE, fixed = FALSE, useBytes = FALSE, invert = FALSE)
{
    pattern <- as.character(pattern)
    if(is.factor(x) && length(levx <- levels(x)) < length(x) &&
       !is.na(pattern[1L]))
    {
        value <- is.character(
            idxna <- suppressWarnings(
                grep(pattern, NA_character_, ignore.case, perl,
                     value, fixed, useBytes, invert)))
        idx <- logical(length(levx))
        idx[grep(pattern, levx, ignore.case, perl,
                 FALSE, fixed, useBytes, invert)] <- TRUE
        idx <- idx[x]
        if(length(idxna)) idx[is.na(x)] <- TRUE
        idx <- which(idx)
        if(value) {
            idx <- x[idx]
            structure(as.character(idx), names=names(idx))
        } else
            idx
    }
    else {
        if(!is.character(x)) x <- structure(as.character(x), names=names(x))
        .Internal(grep(pattern, x, ignore.case, value,
                       perl, fixed, useBytes, invert))
    }
}
