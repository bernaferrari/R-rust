for (op in c('+','-','*','/','^','<','==')) {
  for (value in list(2+0i, 0+1i, complex(0), c(1+2i, 3+4i))) {
    for (left in c(TRUE,FALSE)) {
      expr <- if (left) as.call(list(as.name(op), value, as.name('x'))) else as.call(list(as.name(op), as.name('x'), value))
      dput(expr)
    }
  }
}
e <- as.call(list(as.name('^'), as.name('x'), 2+0i))
dput(D(e, 'x'))
