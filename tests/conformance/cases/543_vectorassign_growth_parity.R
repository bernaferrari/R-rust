# VectorAssign growth: out-of-range subscripts extend the vector with NA
# fill (subscript.c makeSubscript stretch + subassign.c VectorAssign /
# EnlargeVector), NULL-target growth with type coercion (do_subassign_dflt),
# list growth, and character-name stretch via R_UseNamesSymbol.
z <- c(1, 2); z[3] <- 7; print(z)
z2 <- c(1, 2); z2[4] <- 7; print(z2)
n <- NULL; n[2] <- 'z'; print(n); print(n == NA)
n2 <- NULL; n2[1] <- 3.5; print(n2)
n3 <- NULL; n3[3] <- TRUE; print(n3)
l <- c(TRUE, TRUE); l[4] <- TRUE; print(l)
cx <- c(1+2i); cx[3] <- 5i; print(cx)
ch <- c("a", "b"); ch[4] <- "d"; print(is.na(ch)); print(ch[4])
m <- 1:4; m[2] <- NA; m[7] <- 9; print(m)
l4 <- list(1, 2); l4[[4]] <- 9; print(length(l4)); print(l4[[3]] == NULL); print(l4[[4]])
x <- c(a = 1, b = 2); x["c"] <- 3; print(x); print(names(x))
y <- 1:2; names(y) <- c("p", "q"); y["r"] <- 5; print(y)
w <- c(1, 2); w[c(5, 3)] <- c(9, 8); print(w)
d <- c(1, 2); d[3.7] <- 4; print(d)
big <- numeric(0); big[3] <- 1L; print(big)
q <- "aa"; q[3] <- "b"; print(is.na(q[2])); print(nchar(q))
