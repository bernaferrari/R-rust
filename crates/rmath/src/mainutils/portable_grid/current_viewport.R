function() {
  inches <- .rport_grid('current.inches', NULL)
  structure(list(name=.rport_grid('current.name', NULL), x=inches[1], y=inches[2], width=inches[3], height=inches[4]), class='viewport')
}
