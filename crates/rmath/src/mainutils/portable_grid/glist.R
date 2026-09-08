function(...) { z<-list(...); for(x in z) if(!inherits(x,'grob')) stop('only grobs allowed in gList'); structure(z,class='gList') }
