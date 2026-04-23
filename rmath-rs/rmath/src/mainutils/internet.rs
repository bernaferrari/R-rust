#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

use std::os::raw::{c_double, c_int};

use crate::sexp::ffi::SEXP;
use crate::sexp::globals::R_NilValue;

pub  fn Rdownload(_args: SEXP) -> SEXP { R_NilValue()
}
    R_NilValue()
}}

pub type Rconnection = *mut u8;

pub unsafe fn R_newurl(
    _description: *const c_int,
    _mode: *const c_int,
    _headers: SEXP,
    _type: c_int,
) -> Rconnection {
    std::ptr::null_mut()
}

pub unsafe fn R_newsock(
    _host: *const c_int,
    _port: c_int,
    _server: c_int,
    _serverfd: c_int,
    _mode: *const c_int,
    _timeout: c_int,
    _options: c_int,
) -> Rconnection {
    std::ptr::null_mut()
}

pub unsafe fn R_newservsock(_port: c_int) -> Rconnection {
    std::ptr::null_mut()
}

pub unsafe fn extR_HTTPDCreate(_ip: *const c_int, _port: c_int) -> c_int {
    -1
}

pub unsafe fn extR_HTTPDStop() {}

pub  fn Rsockconnect(_sport: SEXP, _shost: SEXP) -> SEXP { R_NilValue()
}
    R_NilValue()
}}

pub  fn Rsockread(_ssock: SEXP, _smaxlen: SEXP) -> SEXP { R_NilValue()
}
    R_NilValue()
}}

pub  fn Rsockclose(_ssock: SEXP) -> SEXP { R_NilValue()
}
    R_NilValue()
}}

pub  fn Rsockopen(_sport: SEXP) -> SEXP { R_NilValue()
}
    R_NilValue()
}}

pub  fn Rsocklisten(_ssock: SEXP) -> SEXP { R_NilValue()
}
    R_NilValue()
}}

pub  fn Rsockwrite(_ssock: SEXP, _sstring: SEXP) -> SEXP { R_NilValue()
}
    R_NilValue()
}}

pub unsafe fn Rsockselect(
    _nsock: c_int,
    _insockfd: *mut c_int,
    _ready: *mut c_int,
    _write: *mut c_int,
    _timeout: c_double,
) -> c_int {
    0
}

pub  fn do_curlVersion(
    _call: SEXP,
    _op: SEXP,
    _args: SEXP,
    _rho: SEXP,
) -> SEXP { R_NilValue()
}
    R_NilValue()
}}

pub  fn do_curlGetHeaders(
    _call: SEXP,
    _op: SEXP,
    _args: SEXP,
    _rho: SEXP,
) -> SEXP { R_NilValue()
}
    R_NilValue()
}}

pub  fn do_curlDownload(
    _call: SEXP,
    _op: SEXP,
    _args: SEXP,
    _rho: SEXP,
) -> SEXP { R_NilValue()
}
    R_NilValue()
}}

pub unsafe fn R_newCurlUrl(
    _description: *const c_int,
    _mode: *const c_int,
    _headers: SEXP,
    _type: c_int,
) -> Rconnection {
    std::ptr::null_mut()
}
