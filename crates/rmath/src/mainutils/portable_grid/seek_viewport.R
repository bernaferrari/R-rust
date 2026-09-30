function(name, recording=TRUE) {
  if (!isTRUE(recording)) stop('unrecorded grid operations are not supported')
  upViewport(0, recording=recording)
  downViewport(name, recording=recording)
}
