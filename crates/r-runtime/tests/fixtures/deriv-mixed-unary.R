for (e in list(quote(-(-x)), quote(-(-y)), quote(-(-x/0)),
               quote(-(-(x/0))), quote(-(-sin(x))), quote(-(-(-sin(x)))),
               quote(+(-(+(-x)))))) {
  dput(e)
  dput(D(e, 'x'))
}
