a <- matrix(c(1+1i, 2+1i, 3+2i, 4+2i), 2, 2)
q <- qr(a)
cat(paste0(
    c(identical(class(q), "qr"), identical(q$rank, 2L), identical(q$pivot, 2:1),
      identical(typeof(q$qr), "complex"), identical(dim(q$qr), c(2L, 2L))),
    collapse=","), "\n", sep="")
cat(isTRUE(all.equal(q$qraux, c(1.52223296786709+0.3481553119113957i,
                                1.99990262856943+0.0139546902488612i),
                     tolerance=1e-12)), "\n", sep="")
cf <- qr.coef(q, c(1+0i, 1+0i))
cat(isTRUE(all.equal(cf, c(-0.4+0.2i, 0.4-0.2i), tolerance=1e-12)), "\n", sep="")
cat(isTRUE(all.equal(as.vector(qr.Q(q)),
                     c(-0.522232967867094-0.3481553119113960i,
                       -0.6963106238227910-0.3481553119113960i,
                       0.7784989441615230-4.2e-16i,
                       -0.6227991553292180+0.0778498944161526i),
                     tolerance=1e-12)), "\n", sep="")
cat(isTRUE(all.equal(as.vector(qr.R(q)),
                     c(-5.744562646538029+0i, 0+0i,
                       -2.611164839335467-0.174077655955698i,
                       -0.389249472080761+0i),
                     tolerance=1e-12)), "\n", sep="")
cat(max(abs(qr.X(q) - a)) < 1e-10, "\n", sep="")
cat(isTRUE(all.equal(qr.qy(q, c(1+1i, 2+2i)),
                     c(1.38292023236735+0.686609608544556i,
                       -1.74945341140214-2.134364457560318i),
                     tolerance=1e-12)), "\n", sep="")
cat(isTRUE(all.equal(qr.qty(q, c(1+1i, 2+2i)),
                     c(-2.959320151246863-0.870388279778489i,
                       -0.311399577664609-0.622799155329218i),
                     tolerance=1e-12)), "\n", sep="")
q3 <- qr(matrix(c(1+1i, 2, 1+2i, 3, 1+1i, 2), 3, 2))
cat(paste0(identical(q3$rank, 2L), ",", identical(q3$pivot, 2:1)), "\n", sep="")
cat(isTRUE(all.equal(qr.coef(q3, c(1, 1, 1)),
                     c(0.252747252747253-0.0879120879120879i,
                       0.252747252747253-0.1098901098901099i),
                     tolerance=1e-12)), "\n", sep="")
e1 <- try(qr.qy(q, c(1, 2)), silent=TRUE)
cat(grepl("complex matrix", e1[1]), "\n", sep="")
e2 <- try(qr.resid(q, c(1+0i, 1+0i)), silent=TRUE)
cat(grepl("not implemented", e2[1]), "\n", sep="")
cat(inherits(try(qr(matrix(1+1i, 2, 2), tol="bogus"), silent=TRUE), "qr"), "\n", sep="")
