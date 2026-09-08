function(x,unitTo,valueOnly=FALSE) { z<-.rport_grid('convert',list(x=x,to=unitTo,axis=1,dimension=FALSE)); if(valueOnly) z else unit(z,unitTo) }
