#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Port of R's src/main/connections.c — R connection infrastructure.
//!
//! Provides a real implementation of R's connection I/O system including
//! files, pipes, URLs, text connections, raw connections, and the
//! associated open/close/read/write/seek/flush operations.
//!
//! Connection objects are stored in the active `RSession`'s connection table.
//! Each connection is identified by an integer index.
//!
//! The connection SEXP is an INTSXP scalar with class c("file","connection")
//! (or "pipe"/"url"/"textConnection"/"rawConnection" as appropriate).

use std::cell::RefCell;
use std::ffi::{CStr, CString};
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::os::raw::{c_double, c_int};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::ptr;

use bzip2::Compression as BzCompression;
use bzip2::read::BzDecoder;
use bzip2::write::BzEncoder;
use flate2::Compression as GzCompression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::context::RError;
use crate::sexp::ffi::{NA_INTEGER, NA_REAL, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::{R_MissingArg, R_NilValue};
use crate::sexp::instance::{RInstance, with_current_instance, with_required_current_instance};
use crate::sexp::protect::*;
mod args;
mod binary;
mod file;
mod lines;
mod pipes;
mod state;

pub use self::args::*;
pub use self::binary::*;
pub use self::file::*;
pub use self::lines::*;
pub use self::pipes::*;
pub use self::state::*;

#[cfg(test)]
mod tests;
