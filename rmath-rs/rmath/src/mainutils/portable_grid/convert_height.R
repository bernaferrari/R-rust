function(x,unitTo,valueOnly=FALSE) { z<-.rport_grid('convert',list(x=x,to=unitTo,axis=1,dimension=TRUE)); if(valueOnly) z else unit(z,unitTo) }
