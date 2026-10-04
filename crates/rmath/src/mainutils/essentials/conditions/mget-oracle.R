# Run with the executable pinned by oracle/r-oracle.json.
run <- function() {
    counter <<- 0L
    envir <- new.env()
    delayedAssign("x", { counter <<- counter + 1L; 42L },
                  eval.env = environment(), assign.env = envir)
    stopifnot(counter == 0L)
    values <- mget("x", envir, inherits = FALSE)
    stopifnot(identical(values, list(x = 42L)), counter == 1L)
    stopifnot(identical(mget(c("x", "x"), envir, inherits = FALSE),
                        structure(list(42L, 42L), names = c("x", "x"))), counter == 1L)
    counter <<- 0L
    envir$y <- 1L
    delayedAssign("x", { counter <<- counter + 1L; envir$y <- 9L; 42L },
                  eval.env = environment(), assign.env = envir)
    stopifnot(identical(mget(c("x", "y"), envir, inherits = FALSE), list(x = 42L, y = 9L)),
              counter == 1L)
    stopifnot(identical(mget("absent", envir, ifnotfound = list(17L)), list(absent = 17L)))
    cat("mget lazy/cache/sequential/fallback GNU controls: PASS\n")
}
run()
contracts <- function() {
    fails <- function(expr) inherits(tryCatch(force(expr), error=identity), 'error')
    e <- new.env(); e$x <- 1L
    stopifnot(fails(mget('x',list(e))), fails(mget('x',NULL)),
              fails(mget('x',e,mode='character')), fails(mget('x',e,mode='bogus')),
              fails(mget('x',e,mode=1L)), fails(mget('x',e,mode=c('any','any'))),
              fails(mget('x',e,inherits=NA)), fails(mget('x',e,inherits=logical(0))))
    parent <- new.env(); parent$x <- 9L; e <- new.env(parent=parent); e$x <- 'wrong'
    stopifnot(identical(mget(c('x','x'),e,mode=c('numeric','integer'),inherits='TRUE'),list(x=9L,x=9L)))
    e <- new.env(); e$y <- 1L
    stopifnot(identical(mget(c('missing','y'),e,ifnotfound=list(function(name){gc();e$y<-9L;name})),list(missing='missing',y=9L)))
    stopifnot(identical(mget(c('a','b'),new.env(),ifnotfound=c(17L,19L)),list(a=17L,b=19L)),
              fails(mget(c('a','b'),new.env(),ifnotfound=list(1L,2L,3L))),
              fails(mget('a',new.env(),ifnotfound=function(x)x)), fails(mget('a',new.env(),ifnotfound=NULL)))
    e <- new.env(); e$x <- c(1L,2L); original <- e$x; names <- c('x','x'); out <- mget(names,e)
    out[[1L]][1L] <- 9L; names(out)[1L] <- 'changed'
    stopifnot(identical(e$x,original),identical(out[[2L]],original),identical(names,c('x','x')))
    cat('mget environment/modes/inherits/callable/length/sharing GNU controls: PASS\n')
}
contracts()
e <- new.env(); e$x <- 19L
stopifnot(identical(mget('x',mode='integer',envir=e),list(x=19L)))
cat('mget exact named matching GNU control: PASS\n')
for (case in list(
 list(quote(mget('',NULL,mode=1L,ifnotfound=function(x)x)), 'invalid name in position 1'),
 list(quote(mget('missing',new.env(),mode=c('any','any'),ifnotfound=function(x)x)), "wrong length for 'mode' argument"),
 list(quote(mget('missing',new.env(),ifnotfound=list(),inherits=NA)), "wrong length for 'ifnotfound' argument"))) {
    error <- tryCatch(eval(case[[1L]]),error=conditionMessage)
    stopifnot(identical(error,case[[2L]]))
}
cat('mget admission ordering GNU control: PASS\n')
