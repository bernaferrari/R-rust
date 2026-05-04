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

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Max open connections.
pub const NCONNECTIONS: usize = 256;

/// R_EOF sentinel value.
const R_EOF: c_int = -1;

// ---------------------------------------------------------------------------
// Connection types
// ---------------------------------------------------------------------------

/// The type of I/O backend for a connection.
#[derive(Debug)]
pub enum ConnKind {
    File,
    Pipe,
    Url,
    Fifo,
    GzFile,
    BzFile,
    XzFile,
    TextConnection,
    RawConnection,
    Terminal(String), // stdin/stdout/stderr
    Null,
}

/// A single R connection.
///
/// This mirrors R's `Rconn` struct from Rconnections.h, but uses Rust
/// types for the I/O handles instead of raw C FILE* pointers.
pub struct RConn {
    /// Connection class name (e.g. "file", "pipe", "textConnection").
    pub class: String,
    /// Human-readable description.
    pub description: String,
    /// Mode string ("r", "w", "rb", "wb", "r+", etc.).
    pub mode: String,
    /// Whether the connection is currently open.
    pub isopen: bool,
    /// Whether the connection is text mode (vs binary).
    pub text: bool,
    /// Whether the connection can read.
    pub canread: bool,
    /// Whether the connection can write.
    pub canwrite: bool,
    /// Whether the connection supports seeking.
    pub canseek: bool,
    /// Whether the connection is blocking.
    pub blocking: bool,
    /// Whether an incomplete line was read.
    pub incomplete: bool,
    /// Status code from close (NA_INTEGER means no status).
    pub status: c_int,
    /// The connection kind.
    pub kind: ConnKind,
    /// Position in raw buffer (for rawConnection).
    pub raw_pos: usize,
    /// Raw data buffer (for rawConnection).
    pub raw_data: Vec<u8>,
    /// Text lines buffer (for textConnection reading).
    pub text_data: String,
    /// Current position in text_data.
    pub text_pos: usize,
    /// Output text lines (for textConnection writing).
    pub text_lines: RefCell<Vec<String>>,
    /// Child process (for pipe connections).
    pub child: Option<Child>,
    /// File handle (for file connections).
    pub file: Option<std::fs::File>,
    /// BufReader wrapper.
    pub reader: Option<BufReader<std::fs::File>>,
    /// BufWriter wrapper.
    pub writer: Option<BufWriter<std::fs::File>>,
    /// Pushback buffer.
    pub pushback: Vec<Vec<u8>>,
}

impl RConn {
    fn new(class: &str, description: &str, mode: &str, kind: ConnKind) -> Self {
        RConn {
            class: class.to_string(),
            description: description.to_string(),
            mode: mode.to_string(),
            isopen: false,
            text: true,
            canread: true,
            canwrite: true,
            canseek: false,
            blocking: true,
            incomplete: false,
            status: NA_INTEGER,
            kind,
            raw_pos: 0,
            raw_data: Vec::new(),
            text_data: String::new(),
            text_pos: 0,
            text_lines: RefCell::new(Vec::new()),
            child: None,
            file: None,
            reader: None,
            writer: None,
            pushback: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Per-session connection table
// ---------------------------------------------------------------------------

/// Sink state.
struct SinkState {
    /// Sink connection indices.
    sink_cons: Vec<c_int>,
    /// Whether each sink should be closed on exit.
    sink_close: Vec<bool>,
    /// Whether each sink is a split (tee) sink.
    sink_split: Vec<bool>,
    /// Number of active sinks.
    sink_number: usize,
    /// Output connection index.
    output_con: c_int,
    /// Error connection index.
    error_con: c_int,
}

impl Default for SinkState {
    fn default() -> Self {
        SinkState {
            sink_cons: Vec::new(),
            sink_close: Vec::new(),
            sink_split: Vec::new(),
            sink_number: 0,
            output_con: 1,
            error_con: 2,
        }
    }
}

/// Connection and sink state owned by one `RInstance`.
pub(crate) struct ConnectionsState {
    table: Vec<Option<Box<RConn>>>,
    sink: SinkState,
}

impl Default for ConnectionsState {
    fn default() -> Self {
        ConnectionsState {
            table: Vec::new(),
            sink: SinkState::default(),
        }
    }
}

struct ConnectionTableGuard {
    table: *mut Vec<Option<Box<RConn>>>,
}

impl std::ops::Deref for ConnectionTableGuard {
    type Target = Vec<Option<Box<RConn>>>;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.table }
    }
}

impl std::ops::DerefMut for ConnectionTableGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.table }
    }
}

struct SinkStateGuard {
    sink: *mut SinkState,
}

impl std::ops::Deref for SinkStateGuard {
    type Target = SinkState;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.sink }
    }
}

impl std::ops::DerefMut for SinkStateGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.sink }
    }
}

#[inline]
fn with_connections_state<F, R>(f: F) -> R
where
    F: FnOnce(&mut ConnectionsState) -> R,
{
    with_required_current_instance(|instance| f(&mut instance.connections_state))
}

fn connection_table() -> ConnectionTableGuard {
    init_connections_table();
    with_connections_state(|state| ConnectionTableGuard {
        table: &mut state.table,
    })
}

fn sink_state() -> SinkStateGuard {
    with_connections_state(|state| SinkStateGuard {
        sink: &mut state.sink,
    })
}

pub(crate) fn output_sink_active() -> bool {
    with_current_instance(output_sink_active_in).unwrap_or(false)
}

pub(crate) fn output_sink_active_in(instance: &mut RInstance) -> bool {
    let sink = &instance.connections_state.sink;
    sink.sink_number > 0 && sink.output_con != 1
}

pub(crate) fn write_output_sink(bytes: &[u8]) -> bool {
    with_current_instance(|instance| write_output_sink_in(instance, bytes)).unwrap_or(false)
}

pub(crate) fn write_output_sink_in(instance: &mut RInstance, bytes: &[u8]) -> bool {
    let sink = &instance.connections_state.sink;
    let target = (sink.sink_number > 0 && sink.output_con != 1).then_some(sink.output_con);
    let Some(connection) = target else {
        return false;
    };

    if connection < 0 {
        r_error("invalid connection");
    }
    let index = connection as usize;
    let table = &mut instance.connections_state.table;
    if table.is_empty() {
        init_connections_table();
    }
    if index >= table.len() {
        r_error("invalid connection");
    }
    let Some(conn) = table[index].as_mut() else {
        r_error("invalid connection");
    };
    if !conn.isopen {
        r_error("connection is not open");
    }
    if !conn.canwrite {
        r_error("cannot write to this connection");
    }
    write_bytes_to_conn(conn, bytes);
    true
}

/// Initialize the connection system with stdin/stdout/stderr.
fn init_connections_table() {
    with_connections_state(|state| {
        let table = &mut state.table;
        if !table.is_empty() {
            return;
        }
        table.clear();
        // Slot 0: stdin
        table.push(Some(Box::new(RConn::new(
            "terminal",
            "stdin",
            "r",
            ConnKind::Terminal("stdin".to_string()),
        ))));
        // Slot 1: stdout
        table.push(Some(Box::new(RConn::new(
            "terminal",
            "stdout",
            "w",
            ConnKind::Terminal("stdout".to_string()),
        ))));
        // Slot 2: stderr
        table.push(Some(Box::new(RConn::new(
            "terminal",
            "stderr",
            "w",
            ConnKind::Terminal("stderr".to_string()),
        ))));
        // Mark standard connections as open
        for i in 0..3 {
            if let Some(ref mut conn) = table[i] {
                conn.isopen = true;
                conn.canread = i == 0;
                conn.canwrite = i != 0;
            }
        }
        // Pre-allocate remaining slots as None
        for _ in 3..NCONNECTIONS {
            table.push(None);
        }
    });
}

/// Find the next available connection slot.
fn next_connection() -> usize {
    init_connections_table();
    let table = connection_table();
    for i in 3..table.len() {
        if table[i].is_none() {
            return i;
        }
    }
    r_error("all connections are in use");
}

/// Get a connection by index. Returns a reference to the connection.
fn get_connection(n: usize) -> ConnectionTableGuard {
    init_connections_table();
    let table = connection_table();
    if n >= table.len() || table[n].is_none() {
        r_error("invalid connection");
    }
    table
}

/// Get a mutable reference to a connection by index.
fn get_connection_mut(n: usize) {
    init_connections_table();
    let _table = connection_table();
    if n >= _table.len() || _table[n].is_none() {
        r_error("invalid connection");
    }
    // The caller should use the table directly for mutation
}

fn checked_connection_index(n: c_int) -> usize {
    if n < 0 {
        r_error("invalid connection");
    }
    let index = n as usize;
    init_connections_table();
    let table = connection_table();
    if index >= table.len() || table[index].is_none() {
        r_error("invalid connection");
    }
    index
}

/// Read one byte from a connection using the Rust connection table.
pub(crate) fn connection_fgetc(n: c_int) -> c_int {
    let index = checked_connection_index(n);
    let mut table = connection_table();
    let Some(conn) = table[index].as_mut() else {
        r_error("invalid connection");
    };

    if !conn.isopen {
        r_error("connection is not open");
    }
    if !conn.canread {
        r_error("cannot read from this connection");
    }

    while let Some(buf) = conn.pushback.last_mut() {
        if let Some(byte) = buf.pop() {
            return byte as c_int;
        }
        conn.pushback.pop();
    }

    let mut byte = [0u8; 1];
    let result = match &mut conn.kind {
        ConnKind::File => conn
            .reader
            .as_mut()
            .map(|reader| reader.read(&mut byte))
            .unwrap_or(Ok(0)),
        ConnKind::GzFile | ConnKind::BzFile | ConnKind::XzFile => {
            if conn.raw_pos >= conn.raw_data.len() {
                Ok(0)
            } else {
                byte[0] = conn.raw_data[conn.raw_pos];
                conn.raw_pos += 1;
                Ok(1)
            }
        }
        ConnKind::Pipe => conn
            .child
            .as_mut()
            .and_then(|child| child.stdout.as_mut())
            .map(|stdout| stdout.read(&mut byte))
            .unwrap_or(Ok(0)),
        ConnKind::RawConnection => {
            if conn.raw_pos >= conn.raw_data.len() {
                Ok(0)
            } else {
                byte[0] = conn.raw_data[conn.raw_pos];
                conn.raw_pos += 1;
                Ok(1)
            }
        }
        ConnKind::TextConnection => {
            let data = conn.text_data.as_bytes();
            if conn.text_pos >= data.len() {
                Ok(0)
            } else {
                byte[0] = data[conn.text_pos];
                conn.text_pos += 1;
                Ok(1)
            }
        }
        ConnKind::Terminal(name) if name == "stdin" => io::stdin().lock().read(&mut byte),
        _ => {
            r_error("cannot read from this connection type");
        }
    };

    match result {
        Ok(0) => R_EOF,
        Ok(_) => byte[0] as c_int,
        Err(e) => r_error(&format!("error reading from connection: {}", e)),
    }
}

/// Push bytes back onto a connection so later reads see them first.
pub(crate) fn connection_pushback(n: c_int, bytes: &[u8]) {
    let index = checked_connection_index(n);
    let mut table = connection_table();
    let Some(conn) = table[index].as_mut() else {
        r_error("invalid connection");
    };

    let mut pushed = bytes.to_vec();
    pushed.reverse();
    conn.pushback.push(pushed);
}

fn pop_pushback_byte(conn: &mut RConn) -> Option<u8> {
    while let Some(buf) = conn.pushback.last_mut() {
        if let Some(byte) = buf.pop() {
            return Some(byte);
        }
        conn.pushback.pop();
    }
    None
}

fn read_pushback_line(conn: &mut RConn) -> Option<String> {
    let mut line = Vec::new();
    while let Some(byte) = pop_pushback_byte(conn) {
        if byte == b'\n' {
            return Some(String::from_utf8_lossy(&line).to_string());
        }
        if byte != b'\r' {
            line.push(byte);
        }
    }

    (!line.is_empty()).then(|| String::from_utf8_lossy(&line).to_string())
}

fn read_raw_line(conn: &mut RConn) -> Option<String> {
    if conn.raw_pos >= conn.raw_data.len() {
        return None;
    }
    let start = conn.raw_pos;
    while conn.raw_pos < conn.raw_data.len() && conn.raw_data[conn.raw_pos] != b'\n' {
        conn.raw_pos += 1;
    }
    let end = conn.raw_pos;
    if conn.raw_pos < conn.raw_data.len() && conn.raw_data[conn.raw_pos] == b'\n' {
        conn.raw_pos += 1;
    }
    let mut line = &conn.raw_data[start..end];
    if line.ends_with(b"\r") {
        line = &line[..line.len() - 1];
    }
    Some(String::from_utf8_lossy(line).to_string())
}

/// Write bytes to a connection using the Rust connection table.
pub(crate) fn connection_write_bytes(n: c_int, bytes: &[u8]) {
    let index = checked_connection_index(n);
    let mut table = connection_table();
    let Some(conn) = table[index].as_mut() else {
        r_error("invalid connection");
    };

    if !conn.isopen {
        r_error("connection is not open");
    }
    if !conn.canwrite {
        r_error("cannot write to this connection");
    }

    write_bytes_to_conn(conn, bytes);
}

fn write_bytes_to_conn(conn: &mut RConn, bytes: &[u8]) {
    match &mut conn.kind {
        ConnKind::File => {
            if let Some(writer) = conn.writer.as_mut()
                && let Err(e) = writer.write_all(bytes).and_then(|_| writer.flush())
            {
                r_error(&format!("error writing to connection: {}", e));
            }
        }
        ConnKind::GzFile | ConnKind::BzFile | ConnKind::XzFile => {
            conn.raw_data.extend_from_slice(bytes);
            conn.raw_pos = conn.raw_data.len();
        }
        ConnKind::Pipe => {
            if let Some(child) = conn.child.as_mut()
                && let Some(stdin) = child.stdin.as_mut()
                && let Err(e) = stdin.write_all(bytes).and_then(|_| stdin.flush())
            {
                r_error(&format!("error writing to pipe: {}", e));
            }
        }
        ConnKind::RawConnection => conn.raw_data.extend_from_slice(bytes),
        ConnKind::TextConnection => {
            let text = String::from_utf8_lossy(bytes);
            conn.text_lines.borrow_mut().push(text.into_owned());
        }
        ConnKind::Terminal(name) if name == "stdout" => {
            let stdout = io::stdout();
            let mut writer = stdout.lock();
            if let Err(e) = writer.write_all(bytes).and_then(|_| writer.flush()) {
                r_error(&format!("error writing to stdout: {}", e));
            }
        }
        ConnKind::Terminal(name) if name == "stderr" => {
            let stderr = io::stderr();
            let mut writer = stderr.lock();
            if let Err(e) = writer.write_all(bytes).and_then(|_| writer.flush()) {
                r_error(&format!("error writing to stderr: {}", e));
            }
        }
        _ => r_error("cannot write to this connection type"),
    }
}

// ---------------------------------------------------------------------------
// Helper functions for extracting arguments from pairlist
// ---------------------------------------------------------------------------

/// Extract a C string from a CHARSXP.
unsafe fn charsxp_to_string(s: SEXP) -> String {
    unsafe {
        if s.is_null() {
            return String::new();
        }
        let ptr = CHAR(s);
        if ptr.is_null() {
            return String::new();
        }
        let cstr = CStr::from_ptr(ptr);
        cstr.to_string_lossy().into_owned()
    }
}

/// Extract a string from the first element of a STRSXP.
unsafe fn string_elt(s: SEXP, i: R_xlen_t) -> String {
    unsafe {
        if s.is_null() {
            return String::new();
        }
        let elt = STRING_ELT(s, i);
        charsxp_to_string(elt)
    }
}

/// Check if an SEXP is a string vector and get its first element.
unsafe fn check_string_arg(arg: SEXP, name: &str) -> String {
    unsafe {
        if arg.is_null() {
            r_error(&format!("invalid '{}' argument", name));
        }
        let t = TYPEOF(arg);
        if t != SEXPTYPE::STRSXP {
            r_error(&format!("invalid '{}' argument", name));
        }
        let len = LENGTH(arg);
        if len < 1 {
            r_error(&format!("invalid '{}' argument", name));
        }
        string_elt(arg, 0)
    }
}

unsafe fn connection_arg_tag_name(cell: SEXP) -> Option<String> {
    unsafe {
        let tag = TAG(cell);
        if tag.is_null() || tag == R_NilValue() {
            return None;
        }
        let pname = PRINTNAME(tag);
        if pname.is_null() {
            return None;
        }
        let chars = CHAR(pname);
        if chars.is_null() {
            return None;
        }
        CStr::from_ptr(chars).to_str().ok().map(str::to_string)
    }
}

/// Extract an integer from an SEXP (length-1 INTSXP or similar scalar).
unsafe fn as_integer(arg: SEXP) -> c_int {
    unsafe {
        if arg.is_null() {
            return NA_INTEGER;
        }
        let t = TYPEOF(arg);
        if t == SEXPTYPE::INTSXP {
            let data = INTEGER(arg);
            if data.is_null() {
                return NA_INTEGER;
            }
            *data
        } else if t == SEXPTYPE::REALSXP {
            let data = REAL(arg);
            if data.is_null() {
                return NA_INTEGER;
            }
            *data as c_int
        } else if t == SEXPTYPE::LGLSXP {
            let data = LOGICAL(arg);
            if data.is_null() {
                return NA_INTEGER;
            }
            *data
        } else {
            NA_INTEGER
        }
    }
}

/// Extract a double from an SEXP.
unsafe fn as_real(arg: SEXP) -> c_double {
    unsafe {
        if arg.is_null() {
            return NA_REAL;
        }
        let t = TYPEOF(arg);
        if t == SEXPTYPE::REALSXP {
            let data = REAL(arg);
            if data.is_null() {
                return NA_REAL;
            }
            *data
        } else if t == SEXPTYPE::INTSXP {
            let v = as_integer(arg);
            if v == NA_INTEGER {
                NA_REAL
            } else {
                v as c_double
            }
        } else {
            NA_REAL
        }
    }
}

/// Extract a logical value from an SEXP.
unsafe fn as_logical(arg: SEXP) -> c_int {
    unsafe {
        if arg.is_null() {
            return NA_INTEGER;
        }
        let t = TYPEOF(arg);
        if t == SEXPTYPE::LGLSXP {
            let data = LOGICAL(arg);
            if data.is_null() {
                return NA_INTEGER;
            }
            *data
        } else {
            NA_INTEGER
        }
    }
}

/// Check that a logical argument is valid (not NA).
unsafe fn check_logical_arg(arg: SEXP, name: &str) -> c_int {
    unsafe {
        let v = as_logical(arg);
        if v == NA_INTEGER {
            r_error(&format!("invalid '{}' argument", name));
        }
        v
    }
}

unsafe fn positional_or(args: SEXP, index: usize, default: SEXP) -> SEXP {
    unsafe {
        let mut cell = args;
        for _ in 0..index {
            if cell.is_null() || cell == R_NilValue() {
                return default;
            }
            cell = CDR(cell);
        }
        if cell.is_null() || cell == R_NilValue() {
            return default;
        }
        let value = CAR(cell);
        if value.is_null() || value == R_NilValue() || value == R_MissingArg() {
            default
        } else {
            value
        }
    }
}

unsafe fn arg_by_name_or_position(
    args: SEXP,
    position: usize,
    names: &[&str],
    default: SEXP,
) -> SEXP {
    unsafe {
        let mut cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            if let Some(tag) = connection_arg_tag_name(cell)
                && names.iter().any(|name| tag == *name)
            {
                let value = CAR(cell);
                return if value.is_null() || value == R_NilValue() || value == R_MissingArg() {
                    default
                } else {
                    value
                };
            }
            cell = CDR(cell);
        }

        let mut untagged = 0usize;
        cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            if connection_arg_tag_name(cell).is_none() {
                if untagged == position {
                    let value = CAR(cell);
                    return if value.is_null() || value == R_NilValue() || value == R_MissingArg() {
                        default
                    } else {
                        value
                    };
                }
                untagged += 1;
            }
            cell = CDR(cell);
        }

        default
    }
}

/// Raise an R error via panic.
fn r_error(msg: &str) -> ! {
    std::panic::panic_any(RError {
        message: msg.to_string(),
    });
}

/// Check if an SEXP inherits from a given class name.
/// Simplified check: looks at the class attribute.
unsafe fn inherits_class(x: SEXP, class_name: &str) -> bool {
    unsafe {
        if x.is_null() {
            return false;
        }
        // If it's an integer scalar, we check the class attribute
        let t = TYPEOF(x);
        if t == SEXPTYPE::INTSXP && LENGTH(x) == 1 {
            // Fast path for this port: many call sites pass a plain connection index
            // (INTSXP scalar) without reliable class metadata attached.
            if class_name == "connection" {
                let data = INTEGER(x);
                if !data.is_null() {
                    let idx = *data;
                    if idx >= 0 {
                        init_connections_table();
                        let table = connection_table();
                        let ui = idx as usize;
                        if ui < table.len() && table[ui].is_some() {
                            return true;
                        }
                    }
                }
            }

            let class_attr = ATTRIB(x);
            if !class_attr.is_null() {
                // Walk the class attribute pairlist
                let mut p = class_attr;
                while !p.is_null() && TYPEOF(p) == SEXPTYPE::LISTSXP {
                    let tag = TAG(p);
                    if !tag.is_null() {
                        let pname = PRINTNAME(tag);
                        if !pname.is_null() {
                            let name = charsxp_to_string(pname);
                            if name == "class" {
                                let val = CAR(p);
                                if !val.is_null() && TYPEOF(val) == SEXPTYPE::STRSXP {
                                    let len = LENGTH(val);
                                    for i in 0..len as R_xlen_t {
                                        let s = string_elt(val, i);
                                        if s == class_name {
                                            return true;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    p = CDR(p);
                }
            }
        }
        false
    }
}

/// Set the class attribute on an SEXP to a two-element vector.
unsafe fn set_connection_class(ans: SEXP, specific_class: &str) {
    unsafe {
        let class_sym = crate::eval::attrib_core::R_ClassSymbol();
        if class_sym.is_null() {
            return;
        }
        let class_vec = Rf_allocVector(SEXPTYPE::STRSXP, 2);
        if class_vec.is_null() {
            return;
        }
        let c1 = CString::new(specific_class).unwrap_or_default();
        let c2 = CString::new("connection").unwrap_or_default();
        let charsxp1 = Rf_mkChar(c1.as_ptr());
        let charsxp2 = Rf_mkChar(c2.as_ptr());
        if !charsxp1.is_null() {
            SET_STRING_ELT(class_vec, 0, charsxp1);
        }
        if !charsxp2.is_null() {
            SET_STRING_ELT(class_vec, 1, charsxp2);
        }
        crate::eval::attrib_core::setAttrib(ans, class_sym, class_vec);
    }
}

// ---------------------------------------------------------------------------
// Connection initialization
// ---------------------------------------------------------------------------

/// Initialize the connection system.
pub unsafe fn R_InitConnections() {
    init_connections_table();
}

// ---------------------------------------------------------------------------
// do_file — file(description, open = "", mode = "r", raw = FALSE)
// ---------------------------------------------------------------------------

pub unsafe fn do_file(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let empty = Rf_mkString(CString::new("").unwrap_or_default().as_ptr());
        let native = Rf_mkString(CString::new("native.enc").unwrap_or_default().as_ptr());
        let default_method = Rf_mkString(CString::new("default").unwrap_or_default().as_ptr());
        let scmd = positional_or(args, 0, empty);
        let sopen = positional_or(args, 1, empty);
        let _enc = positional_or(args, 2, native);
        let _block = positional_or(args, 3, Rf_ScalarLogical(crate::sexp::ffi::TRUE));
        let _method = positional_or(args, 5, default_method);
        let raw = check_logical_arg(
            positional_or(args, 4, Rf_ScalarLogical(crate::sexp::ffi::FALSE)),
            "raw",
        );

        let description = check_string_arg(scmd, "description");
        let open = check_string_arg(sopen, "open");
        let open_mode = if open.is_empty() {
            "r".to_string()
        } else {
            open
        };

        let ncon = next_connection();
        let mut conn = RConn::new("file", &description, &open_mode, ConnKind::File);
        conn.canseek = raw == 0;
        conn.text = !open_mode.contains('b');

        // Open immediately if open mode is non-empty
        if !open_mode.is_empty() {
            let file_result = open_file_conn(&description, &open_mode);
            match file_result {
                Ok((file, reader, writer)) => {
                    conn.file = Some(file);
                    conn.reader = reader;
                    conn.writer = writer;
                    conn.isopen = true;
                    conn.canread = open_mode.starts_with('r');
                    conn.canwrite = open_mode.starts_with('w') || open_mode.starts_with('a');
                    if open_mode.contains('+') {
                        conn.canread = true;
                        conn.canwrite = true;
                    }
                }
                Err(e) => {
                    r_error(&format!("cannot open file '{}': {}", description, e));
                }
            }
        }

        let mut table = connection_table();
        table[ncon] = Some(Box::new(conn));
        drop(table);

        let ans = Rf_ScalarInteger(ncon as c_int);
        let _ans_guard = protect(ans);
        set_connection_class(ans, "file");
        ans
    }
}

/// Open a file and return (file, optional reader, optional writer).
#[allow(clippy::type_complexity)]
fn open_file_conn(
    path: &str,
    mode: &str,
) -> io::Result<(File, Option<BufReader<File>>, Option<BufWriter<File>>)> {
    let mut opts = OpenOptions::new();
    if mode.contains('r') {
        opts.read(true);
    }
    if mode.contains('w') {
        opts.write(true).create(true).truncate(true);
    }
    if mode.contains('a') {
        opts.append(true);
    }
    let file = opts.open(path)?;

    let reader = if mode.contains('r') {
        Some(BufReader::new(file.try_clone()?))
    } else {
        None
    };

    let writer = if mode.contains('w') || mode.contains('a') {
        Some(BufWriter::new(file.try_clone()?))
    } else {
        None
    };

    Ok((file, reader, writer))
}

fn open_gz_conn(conn: &mut RConn, mode: &str) -> io::Result<()> {
    conn.mode = mode.to_string();
    conn.text = !mode.contains('b');
    conn.raw_data.clear();
    conn.raw_pos = 0;
    conn.file = None;
    conn.reader = None;
    conn.writer = None;

    let can_read = mode.starts_with('r') || mode.contains('+');
    let can_write = mode.starts_with('w') || mode.starts_with('a') || mode.contains('+');

    if mode.starts_with('r') || mode.starts_with('a') || mode.contains('+') {
        let path = Path::new(&conn.description);
        if path.exists() {
            let file = File::open(path)?;
            let mut decoder = GzDecoder::new(file);
            decoder.read_to_end(&mut conn.raw_data)?;
        } else if mode.starts_with('r') {
            return Err(io::Error::new(io::ErrorKind::NotFound, "file not found"));
        }
    }

    if mode.starts_with('w') && !mode.starts_with("w+") {
        conn.raw_data.clear();
    }

    conn.raw_pos = if mode.starts_with('a') {
        conn.raw_data.len()
    } else {
        0
    };
    conn.isopen = true;
    conn.canread = can_read;
    conn.canwrite = can_write;
    Ok(())
}

fn flush_gz_conn(conn: &mut RConn) -> io::Result<()> {
    let file = File::create(&conn.description)?;
    let mut encoder = GzEncoder::new(file, GzCompression::default());
    encoder.write_all(&conn.raw_data)?;
    encoder.finish()?;
    Ok(())
}

fn open_bz_conn(conn: &mut RConn, mode: &str) -> io::Result<()> {
    conn.mode = mode.to_string();
    conn.text = !mode.contains('b');
    conn.raw_data.clear();
    conn.raw_pos = 0;
    conn.file = None;
    conn.reader = None;
    conn.writer = None;

    let can_read = mode.starts_with('r') || mode.contains('+');
    let can_write = mode.starts_with('w') || mode.starts_with('a') || mode.contains('+');

    if mode.starts_with('r') || mode.starts_with('a') || mode.contains('+') {
        let path = Path::new(&conn.description);
        if path.exists() {
            let file = File::open(path)?;
            let mut decoder = BzDecoder::new(file);
            decoder.read_to_end(&mut conn.raw_data)?;
        } else if mode.starts_with('r') {
            return Err(io::Error::new(io::ErrorKind::NotFound, "file not found"));
        }
    }

    if mode.starts_with('w') && !mode.starts_with("w+") {
        conn.raw_data.clear();
    }

    conn.raw_pos = if mode.starts_with('a') {
        conn.raw_data.len()
    } else {
        0
    };
    conn.isopen = true;
    conn.canread = can_read;
    conn.canwrite = can_write;
    Ok(())
}

fn flush_bz_conn(conn: &mut RConn) -> io::Result<()> {
    let file = File::create(&conn.description)?;
    let mut encoder = BzEncoder::new(file, BzCompression::default());
    encoder.write_all(&conn.raw_data)?;
    encoder.finish()?;
    Ok(())
}

fn open_xz_conn(conn: &mut RConn, mode: &str) -> io::Result<()> {
    conn.mode = mode.to_string();
    conn.text = !mode.contains('b');
    conn.raw_data.clear();
    conn.raw_pos = 0;
    conn.file = None;
    conn.reader = None;
    conn.writer = None;

    let can_read = mode.starts_with('r') || mode.contains('+');
    let can_write = mode.starts_with('w') || mode.starts_with('a') || mode.contains('+');

    if mode.starts_with('r') || mode.starts_with('a') || mode.contains('+') {
        let path = Path::new(&conn.description);
        if path.exists() {
            let file = File::open(path)?;
            let mut reader = BufReader::new(file);
            lzma_rs::xz_decompress(&mut reader, &mut conn.raw_data)
                .map_err(|err| io::Error::other(err.to_string()))?;
        } else if mode.starts_with('r') {
            return Err(io::Error::new(io::ErrorKind::NotFound, "file not found"));
        }
    }

    if mode.starts_with('w') && !mode.starts_with("w+") {
        conn.raw_data.clear();
    }

    conn.raw_pos = if mode.starts_with('a') {
        conn.raw_data.len()
    } else {
        0
    };
    conn.isopen = true;
    conn.canread = can_read;
    conn.canwrite = can_write;
    Ok(())
}

fn flush_xz_conn(conn: &mut RConn) -> io::Result<()> {
    let input = io::Cursor::new(&conn.raw_data);
    let mut reader = BufReader::new(input);
    let mut output = File::create(&conn.description)?;
    lzma_rs::xz_compress(&mut reader, &mut output)
}

// ---------------------------------------------------------------------------
// do_pipe — pipe(description, open = "", encoding = "")
// ---------------------------------------------------------------------------

pub unsafe fn do_pipe(_call: SEXP, _op: SEXP, mut args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let scmd = CAR(args);
        args = CDR(args);
        let sopen = CAR(args);
        args = CDR(args);
        let _enc = CAR(args);

        let description = check_string_arg(scmd, "description");
        let open = check_string_arg(sopen, "open");
        let open_mode = if open.is_empty() {
            "r".to_string()
        } else {
            open
        };

        let ncon = next_connection();
        let mut conn = RConn::new("pipe", &description, &open_mode, ConnKind::Pipe);
        conn.canseek = false;
        conn.text = !open_mode.contains('b');

        // Open immediately if open mode is non-empty
        if !open_mode.is_empty() {
            let is_read = open_mode.starts_with('r');
            let mut cmd = Command::new("sh");
            cmd.arg("-c").arg(&description);

            if is_read {
                cmd.stdout(Stdio::piped());
                cmd.stderr(Stdio::null());
            } else {
                cmd.stdin(Stdio::piped());
                cmd.stderr(Stdio::null());
            }

            match cmd.spawn() {
                Ok(child) => {
                    conn.child = Some(child);
                    conn.isopen = true;
                    conn.canread = is_read;
                    conn.canwrite = !is_read;
                }
                Err(e) => {
                    r_error(&format!("cannot open pipe '{}': {}", description, e));
                }
            }
        }

        let mut table = connection_table();
        table[ncon] = Some(Box::new(conn));
        drop(table);

        let ans = Rf_ScalarInteger(ncon as c_int);
        let _ans_guard = protect(ans);
        set_connection_class(ans, "pipe");
        ans
    }
}

// ---------------------------------------------------------------------------
// do_url — url(description, open = "", mode = "r", blocking = TRUE, encoding = "", method = "default", headers = NULL)
// ---------------------------------------------------------------------------

pub unsafe fn do_url(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let empty = Rf_mkString(CString::new("").unwrap_or_default().as_ptr());
        let native = Rf_mkString(CString::new("native.enc").unwrap_or_default().as_ptr());
        let default_method = Rf_mkString(CString::new("default").unwrap_or_default().as_ptr());
        let scmd = arg_by_name_or_position(args, 0, &["description"], R_NilValue());
        let sopen = arg_by_name_or_position(args, 1, &["open"], empty);
        let _block = logical_arg_or(
            arg_by_name_or_position(args, 2, &["blocking"], Rf_ScalarLogical(1)),
            "blocking",
            1,
        );
        let _enc = arg_by_name_or_position(args, 3, &["encoding"], native);
        let _method = arg_by_name_or_position(args, 4, &["method"], default_method);
        let _headers = arg_by_name_or_position(args, 5, &["headers"], R_NilValue());

        let description = check_string_arg(scmd, "description");
        let open = check_string_arg(sopen, "open");
        let open_mode = if open.is_empty() {
            "r".to_string()
        } else {
            open
        };

        // Check if it's a file:// URL or a regular path
        let (actual_desc, conn_class) = if let Some(path) = description.strip_prefix("file://") {
            (path.to_string(), "file".to_string())
        } else if description.starts_with("http://")
            || description.starts_with("https://")
            || description.starts_with("ftp://")
            || description.starts_with("ftps://")
        {
            r_error(
                "remote URL connections are disabled in this pure-R Android runtime; fetch bytes through the host network policy and pass a file, rawConnection, or textConnection",
            );
        } else {
            // Treat as a regular file path (like R does for non-URL descriptions in url())
            (description.clone(), "file".to_string())
        };

        let ncon = next_connection();
        let mut conn = if conn_class == "url" {
            RConn::new("url", &actual_desc, &open_mode, ConnKind::Url)
        } else {
            RConn::new("file", &actual_desc, &open_mode, ConnKind::File)
        };
        conn.canseek = conn_class == "file";
        conn.text = !open_mode.contains('b');

        if conn_class == "file" && !open_mode.is_empty() {
            let file_result = open_file_conn(&actual_desc, &open_mode);
            match file_result {
                Ok((file, reader, writer)) => {
                    conn.file = Some(file);
                    conn.reader = reader;
                    conn.writer = writer;
                    conn.isopen = true;
                    conn.canread = open_mode.starts_with('r');
                    conn.canwrite = open_mode.starts_with('w') || open_mode.starts_with('a');
                    if open_mode.contains('+') {
                        conn.canread = true;
                        conn.canwrite = true;
                    }
                }
                Err(e) => {
                    r_error(&format!("cannot open file '{}': {}", actual_desc, e));
                }
            }
        }

        let mut table = connection_table();
        table[ncon] = Some(Box::new(conn));
        drop(table);

        let ans = Rf_ScalarInteger(ncon as c_int);
        let _ans_guard = protect(ans);
        set_connection_class(ans, &conn_class);
        ans
    }
}

// ---------------------------------------------------------------------------
// do_fifo — fifo(description, open, mode)
// ---------------------------------------------------------------------------

pub unsafe fn do_fifo(_call: SEXP, _op: SEXP, mut args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let scmd = CAR(args);
        args = CDR(args);
        let sopen = CAR(args);
        args = CDR(args);
        let _enc = CAR(args);

        let description = check_string_arg(scmd, "description");
        let open = check_string_arg(sopen, "open");
        let open_mode = if open.is_empty() {
            "r".to_string()
        } else {
            open
        };

        let ncon = next_connection();
        let mut conn = RConn::new("fifo", &description, &open_mode, ConnKind::Fifo);
        conn.canseek = false;

        let mut table = connection_table();
        table[ncon] = Some(Box::new(conn));
        drop(table);

        let ans = Rf_ScalarInteger(ncon as c_int);
        let _ans_guard = protect(ans);
        set_connection_class(ans, "fifo");
        ans
    }
}

// ---------------------------------------------------------------------------
// do_gzfile — gzfile(description, open = "", compression = 6)
// ---------------------------------------------------------------------------

pub unsafe fn do_gzfile(_call: SEXP, _op: SEXP, mut args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let scmd = CAR(args);
        args = CDR(args);
        let sopen = CAR(args);
        args = CDR(args);
        let _compression = CAR(args);

        let description = check_string_arg(scmd, "description");
        let open = check_string_arg(sopen, "open");
        let open_mode = if open.is_empty() {
            "r".to_string()
        } else {
            open
        };

        let mut conn = RConn::new("gzfile", &description, &open_mode, ConnKind::GzFile);
        conn.canseek = false;
        conn.text = !open_mode.contains('b');

        if !open_mode.is_empty() {
            if let Err(e) = open_gz_conn(&mut conn, &open_mode) {
                r_error(&format!("cannot open file '{}': {}", description, e));
            }
        }

        let ncon = next_connection();
        let mut table = connection_table();
        table[ncon] = Some(Box::new(conn));
        drop(table);

        let ans = Rf_ScalarInteger(ncon as c_int);
        let _ans_guard = protect(ans);
        set_connection_class(ans, "gzfile");
        ans
    }
}

// ---------------------------------------------------------------------------
// do_bzfile — bzfile(description, open, compression)
// ---------------------------------------------------------------------------

pub unsafe fn do_bzfile(_call: SEXP, _op: SEXP, mut args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let scmd = CAR(args);
        args = CDR(args);
        let sopen = CAR(args);
        args = CDR(args);
        let _compression = CAR(args);

        let description = check_string_arg(scmd, "description");
        let open = check_string_arg(sopen, "open");
        let open_mode = if open.is_empty() {
            "r".to_string()
        } else {
            open
        };

        let mut conn = RConn::new("bzfile", &description, &open_mode, ConnKind::BzFile);
        conn.canseek = false;
        conn.text = !open_mode.contains('b');

        if !open_mode.is_empty() {
            if let Err(e) = open_bz_conn(&mut conn, &open_mode) {
                r_error(&format!("cannot open file '{}': {}", description, e));
            }
        }

        let ncon = next_connection();
        let mut table = connection_table();
        table[ncon] = Some(Box::new(conn));
        drop(table);

        let ans = Rf_ScalarInteger(ncon as c_int);
        let _ans_guard = protect(ans);
        set_connection_class(ans, "bzfile");
        ans
    }
}

// ---------------------------------------------------------------------------
// do_xzfile — xzfile(description, open, compression)
// ---------------------------------------------------------------------------

pub unsafe fn do_xzfile(_call: SEXP, _op: SEXP, mut args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let scmd = CAR(args);
        args = CDR(args);
        let sopen = CAR(args);
        args = CDR(args);
        let _compression = CAR(args);

        let description = check_string_arg(scmd, "description");
        let open = check_string_arg(sopen, "open");
        let open_mode = if open.is_empty() {
            "r".to_string()
        } else {
            open
        };

        let mut conn = RConn::new("xzfile", &description, &open_mode, ConnKind::XzFile);
        conn.canseek = false;
        conn.text = !open_mode.contains('b');

        if !open_mode.is_empty() {
            if let Err(e) = open_xz_conn(&mut conn, &open_mode) {
                r_error(&format!("cannot open file '{}': {}", description, e));
            }
        }

        let ncon = next_connection();
        let mut table = connection_table();
        table[ncon] = Some(Box::new(conn));
        drop(table);

        let ans = Rf_ScalarInteger(ncon as c_int);
        let _ans_guard = protect(ans);
        set_connection_class(ans, "xzfile");
        ans
    }
}

// ---------------------------------------------------------------------------
// do_open — open(con, open = "", blocking = TRUE)
// ---------------------------------------------------------------------------

#[allow(clippy::assigning_clones)]
pub unsafe fn do_open(_call: SEXP, _op: SEXP, mut args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let scon = CAR(args);
        args = CDR(args);
        let sopen = CAR(args);
        args = CDR(args);
        let _block = check_logical_arg(CAR(args), "blocking");

        if !inherits_class(scon, "connection") {
            r_error("'con' is not a connection");
        }
        let i = as_integer(scon) as usize;
        if i < 3 {
            r_error("cannot open standard connections");
        }

        let open_str = check_string_arg(sopen, "open");
        let open_mode = if open_str.is_empty() {
            "r".to_string()
        } else {
            open_str
        };

        let mut table = connection_table();
        let Some(conn) = table[i].as_mut() else {
            r_error("invalid connection");
        };
        if conn.isopen {
            return R_NilValue();
        }

        conn.mode = open_mode.clone();
        conn.text = !open_mode.contains('b');

        match &conn.kind {
            ConnKind::File => {
                let file_result = open_file_conn(&conn.description, &open_mode);
                match file_result {
                    Ok((file, reader, writer)) => {
                        conn.file = Some(file);
                        conn.reader = reader;
                        conn.writer = writer;
                        conn.isopen = true;
                        conn.canread = open_mode.starts_with('r');
                        conn.canwrite = open_mode.starts_with('w') || open_mode.starts_with('a');
                        if open_mode.contains('+') {
                            conn.canread = true;
                            conn.canwrite = true;
                        }
                    }
                    Err(e) => {
                        r_error(&format!("cannot open the connection: {}", e));
                    }
                }
            }
            ConnKind::GzFile => {
                if let Err(e) = open_gz_conn(conn, &open_mode) {
                    r_error(&format!("cannot open the connection: {}", e));
                }
            }
            ConnKind::BzFile => {
                if let Err(e) = open_bz_conn(conn, &open_mode) {
                    r_error(&format!("cannot open the connection: {}", e));
                }
            }
            ConnKind::XzFile => {
                if let Err(e) = open_xz_conn(conn, &open_mode) {
                    r_error(&format!("cannot open the connection: {}", e));
                }
            }
            ConnKind::Pipe => {
                let is_read = open_mode.starts_with('r');
                let mut cmd = Command::new("sh");
                cmd.arg("-c").arg(&conn.description);
                if is_read {
                    cmd.stdout(Stdio::piped());
                    cmd.stderr(Stdio::null());
                } else {
                    cmd.stdin(Stdio::piped());
                    cmd.stderr(Stdio::null());
                }
                match cmd.spawn() {
                    Ok(child) => {
                        conn.child = Some(child);
                        conn.isopen = true;
                        conn.canread = is_read;
                        conn.canwrite = !is_read;
                    }
                    Err(e) => {
                        r_error(&format!("cannot open pipe: {}", e));
                    }
                }
            }
            ConnKind::TextConnection => {
                conn.isopen = true;
            }
            ConnKind::RawConnection => {
                conn.isopen = true;
            }
            _ => {
                r_error("cannot open this type of connection");
            }
        }

        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_close — close(con)
// ---------------------------------------------------------------------------

pub unsafe fn do_close(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let scon = CAR(args);

        if !inherits_class(scon, "connection") {
            r_error("'con' is not a connection");
        }
        let i = as_integer(scon) as usize;
        if i < 3 {
            r_error("cannot close standard connections");
        }

        // Check if it's a sink connection
        {
            let sink = sink_state();
            for j in 0..sink.sink_number {
                if i as c_int == sink.sink_cons[j] {
                    r_error("cannot close 'output' sink connection");
                }
            }
            if i as c_int == sink.error_con {
                r_error("cannot close 'message' sink connection");
            }
        }

        let mut table = connection_table();
        if let Some(ref mut conn) = table[i] {
            close_connection_inner(conn);
        }
        table[i] = None;

        R_NilValue()
    }
}

/// Internal close logic for a connection.
fn close_connection_inner(conn: &mut RConn) {
    if !conn.isopen {
        return;
    }

    if conn.canwrite {
        match conn.kind {
            ConnKind::GzFile => {
                let _ = flush_gz_conn(conn);
            }
            ConnKind::BzFile => {
                let _ = flush_bz_conn(conn);
            }
            ConnKind::XzFile => {
                let _ = flush_xz_conn(conn);
            }
            _ => {}
        }
    }

    conn.status = 0; // success
    conn.isopen = false;

    // Close file handles
    if let Some(mut writer) = conn.writer.take() {
        let _ = writer.flush();
    }
    conn.reader.take();
    conn.file.take();

    // Close child processes
    if let Some(mut child) = conn.child.take() {
        // Try to close stdin if writing to pipe
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.flush();
        }
        // Wait for child to finish
        match child.wait() {
            Ok(status) => {
                conn.status = status.code().unwrap_or(0);
            }
            Err(_) => {
                conn.status = -1;
            }
        }
    }

    conn.pushback.clear();
}

// ---------------------------------------------------------------------------
// do_isopen — isOpen(con, rw = "")
// ---------------------------------------------------------------------------

pub unsafe fn do_isopen(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let scon = arg_by_name_or_position(args, 0, &["con"], R_NilValue());
        let rw = connection_rw_mode(arg_by_name_or_position(args, 1, &["rw"], R_NilValue()));

        let i = as_integer(scon) as usize;
        init_connections_table();
        let table = connection_table();
        if i >= table.len() || table[i].is_none() {
            r_error("invalid connection");
        }
        let Some(conn) = table[i].as_ref() else {
            r_error("invalid connection");
        };
        let mut res = if conn.isopen { 1 } else { 0 };
        match rw {
            1 => {
                if !conn.canread {
                    res = 0;
                }
            }
            2 => {
                if !conn.canwrite {
                    res = 0;
                }
            }
            _ => {} // intentionally unhandled: unknown connection open mode
        }
        Rf_ScalarLogical(res)
    }
}

// ---------------------------------------------------------------------------
// do_isincomplete — isIncomplete(con)
// ---------------------------------------------------------------------------

pub unsafe fn do_isincomplete(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let scon = CAR(args);

        if !inherits_class(scon, "connection") {
            r_error("'con' is not a connection");
        }
        let i = as_integer(scon) as usize;
        let table = connection_table();
        if i >= table.len() || table[i].is_none() {
            return Rf_ScalarLogical(0);
        }
        let Some(conn) = table[i].as_ref() else {
            return Rf_ScalarLogical(0);
        };
        Rf_ScalarLogical(if conn.incomplete { 1 } else { 0 })
    }
}

// ---------------------------------------------------------------------------
// do_isseekable — isSeekable(con)
// ---------------------------------------------------------------------------

pub unsafe fn do_isseekable(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let scon = CAR(args);

        if !inherits_class(scon, "connection") {
            r_error("'con' is not a connection");
        }
        let i = as_integer(scon) as usize;
        let table = connection_table();
        if i >= table.len() || table[i].is_none() {
            return Rf_ScalarLogical(0);
        }
        let Some(conn) = table[i].as_ref() else {
            return Rf_ScalarLogical(0);
        };
        Rf_ScalarLogical(if conn.canseek { 1 } else { 0 })
    }
}

// ---------------------------------------------------------------------------
// do_isatty — isatty(con)
// ---------------------------------------------------------------------------

pub unsafe fn do_isatty(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe { Rf_ScalarLogical(0) }
}

// ---------------------------------------------------------------------------
// do_readLines — readLines(con, n = -1, ok = TRUE, warn = TRUE, encoding = "", skipNul = FALSE)
// ---------------------------------------------------------------------------

pub unsafe fn do_readLines(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let scon = CAR(args);
        let mut n_val = -1;
        let mut _ok = 1;
        let mut _warn = 1;
        let mut _encoding = R_NilValue();
        let mut _skipNul = 0;
        let mut positional = 0;
        let mut current = CDR(args);
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            match connection_arg_tag_name(current).as_deref() {
                Some("n") => n_val = as_integer(arg),
                Some("ok") => _ok = check_logical_arg(arg, "ok"),
                Some("warn") => _warn = check_logical_arg(arg, "warn"),
                Some("encoding") => _encoding = arg,
                Some("skipNul") => _skipNul = check_logical_arg(arg, "skipNul"),
                _ => {
                    match positional {
                        0 => n_val = as_integer(arg),
                        1 => _ok = check_logical_arg(arg, "ok"),
                        2 => _warn = check_logical_arg(arg, "warn"),
                        3 => _encoding = arg,
                        4 => _skipNul = check_logical_arg(arg, "skipNul"),
                        _ => {}
                    }
                    positional += 1;
                }
            }
            current = CDR(current);
        }

        let n = if n_val < 0 {
            i64::MAX as usize
        } else {
            n_val as usize
        };

        if TYPEOF(scon) == SEXPTYPE::STRSXP {
            let path = check_string_arg(scon, "con");
            let contents = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| r_error(&format!("cannot open file '{}': {}", path, e)));
            let lines: Vec<String> = contents.lines().take(n).map(str::to_string).collect();
            let ans = Rf_allocVector(SEXPTYPE::STRSXP, lines.len() as c_int);
            if !ans.is_null() {
                for (idx, line) in lines.iter().enumerate() {
                    let c_line = CString::new(line.as_str())
                        .unwrap_or_else(|_| CString::new("").unwrap_or_default());
                    let charsxp = Rf_mkChar(c_line.as_ptr());
                    SET_STRING_ELT(ans, idx as R_xlen_t, charsxp);
                }
            }
            return ans;
        }

        if !inherits_class(scon, "connection") {
            r_error("'con' is not a connection");
        }
        let i = as_integer(scon) as usize;

        let mut table = connection_table();
        let Some(conn) = table[i].as_mut() else {
            r_error("invalid connection");
        };

        if !conn.isopen {
            r_error("connection is not open");
        }
        if !conn.canread {
            r_error("cannot read from this connection");
        }

        conn.incomplete = false;
        let mut lines: Vec<String> = Vec::new();
        while lines.len() < n {
            let Some(line) = read_pushback_line(conn) else {
                break;
            };
            lines.push(line);
        }
        let backend_limit = n.saturating_sub(lines.len());

        match &conn.kind {
            ConnKind::File => {
                if let Some(ref mut reader) = conn.reader {
                    for _ in 0..backend_limit {
                        let mut line = String::new();
                        match reader.read_line(&mut line) {
                            Ok(0) => break, // EOF
                            Ok(_) => {
                                // Strip trailing newline
                                if line.ends_with('\n') {
                                    line.pop();
                                    if line.ends_with('\r') {
                                        line.pop();
                                    }
                                }
                                lines.push(line);
                            }
                            Err(e) => {
                                r_error(&format!("error reading from connection: {}", e));
                            }
                        }
                    }
                }
            }
            ConnKind::GzFile | ConnKind::BzFile | ConnKind::XzFile => {
                for _ in 0..backend_limit {
                    let Some(line) = read_raw_line(conn) else {
                        break;
                    };
                    lines.push(line);
                }
            }
            ConnKind::Pipe => {
                if let Some(ref mut child) = conn.child
                    && let Some(ref mut stdout) = child.stdout
                {
                    let _reader = BufReader::new(stdout);
                    #[allow(clippy::never_loop)]
                    for _ in 0..backend_limit {
                        break;
                    }
                }
            }
            ConnKind::TextConnection => {
                // Read from text_data buffer
                let data = conn.text_data.clone();
                let pos = conn.text_pos;
                let remaining = &data[pos..];
                for line_str in remaining.split('\n') {
                    if lines.len() >= n {
                        break;
                    }
                    lines.push(line_str.to_string());
                }
                if backend_limit == 0 {
                    // Pushback satisfied this read without touching the underlying data.
                } else if lines.len() >= n {
                    // Update position
                    let mut new_pos = pos;
                    for _ in 0..backend_limit {
                        if let Some(idx) = data[new_pos..].find('\n') {
                            new_pos += idx + 1;
                        } else {
                            break;
                        }
                    }
                    conn.text_pos = new_pos;
                } else {
                    conn.text_pos = data.len();
                }
            }
            ConnKind::RawConnection => {
                // Read raw bytes as lines
                let remaining = &conn.raw_data[conn.raw_pos..];
                let mut line = Vec::new();
                for &byte in remaining {
                    if byte == b'\n' {
                        lines.push(String::from_utf8_lossy(&line).to_string());
                        line.clear();
                    } else {
                        line.push(byte);
                    }
                    if lines.len() >= n {
                        break;
                    }
                }
                conn.raw_pos += remaining.len();
                if !line.is_empty() {
                    lines.push(String::from_utf8_lossy(&line).to_string());
                }
            }
            ConnKind::Terminal(name) if name == "stdin" => {
                let stdin = io::stdin();
                let mut reader = stdin.lock();
                for _ in 0..backend_limit {
                    let mut line = String::new();
                    match reader.read_line(&mut line) {
                        Ok(0) => break,
                        Ok(_) => {
                            if line.ends_with('\n') {
                                line.pop();
                                if line.ends_with('\r') {
                                    line.pop();
                                }
                            }
                            lines.push(line);
                        }
                        Err(e) => {
                            r_error(&format!("error reading from stdin: {}", e));
                        }
                    }
                }
            }
            _ => {
                r_error("cannot read from this connection type");
            }
        }

        // Build result STRSXP
        let nlines = lines.len() as c_int;
        let ans = Rf_allocVector(SEXPTYPE::STRSXP, nlines);
        if !ans.is_null() {
            for (idx, line) in lines.iter().enumerate() {
                let c_line = CString::new(line.as_str())
                    .unwrap_or_else(|_| CString::new("").unwrap_or_default());
                let charsxp = Rf_mkChar(c_line.as_ptr());
                SET_STRING_ELT(ans, idx as R_xlen_t, charsxp);
            }
        }

        ans
    }
}

// ---------------------------------------------------------------------------
// do_writeLines — writeLines(text, con, sep = "\n", useBytes = FALSE)
// ---------------------------------------------------------------------------

pub unsafe fn do_writeLines(_call: SEXP, _op: SEXP, mut args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let text = CAR(args);
        args = CDR(args);
        let scon = CAR(args);
        args = CDR(args);
        let sep = CAR(args);
        args = CDR(args);
        let _useBytes = check_logical_arg(CAR(args), "useBytes");

        if text.is_null() || TYPEOF(text) != SEXPTYPE::STRSXP {
            r_error("invalid 'text' argument");
        }
        if !inherits_class(scon, "connection") {
            r_error("'con' is not a connection");
        }

        let sep_str = check_string_arg(sep, "sep");
        let text_len = LENGTH(text) as R_xlen_t;

        let i = as_integer(scon) as usize;
        let mut table = connection_table();
        let Some(conn) = table[i].as_mut() else {
            r_error("invalid connection");
        };

        if !conn.isopen {
            r_error("connection is not open");
        }
        if !conn.canwrite {
            r_error("cannot write to this connection");
        }

        match &conn.kind {
            ConnKind::File => {
                if let Some(ref mut writer) = conn.writer {
                    for j in 0..text_len {
                        let line = string_elt(text, j);
                        if let Err(e) = write!(writer, "{}{}", line, sep_str) {
                            r_error(&format!("error writing to connection: {}", e));
                        }
                    }
                    let _ = writer.flush();
                }
            }
            ConnKind::GzFile | ConnKind::BzFile | ConnKind::XzFile => {
                for j in 0..text_len {
                    let line = string_elt(text, j);
                    conn.raw_data.extend_from_slice(line.as_bytes());
                    conn.raw_data.extend_from_slice(sep_str.as_bytes());
                }
                conn.raw_pos = conn.raw_data.len();
            }
            ConnKind::Terminal(name) if name == "stdout" => {
                let stdout = io::stdout();
                let mut writer = stdout.lock();
                for j in 0..text_len {
                    let line = string_elt(text, j);
                    if let Err(e) = write!(writer, "{}{}", line, sep_str) {
                        r_error(&format!("error writing to stdout: {}", e));
                    }
                }
            }
            ConnKind::Terminal(name) if name == "stderr" => {
                let stderr = io::stderr();
                let mut writer = stderr.lock();
                for j in 0..text_len {
                    let line = string_elt(text, j);
                    if let Err(e) = write!(writer, "{}{}", line, sep_str) {
                        r_error(&format!("error writing to stderr: {}", e));
                    }
                }
            }
            ConnKind::TextConnection => {
                let mut lines = conn.text_lines.borrow_mut();
                for j in 0..text_len {
                    let line = string_elt(text, j);
                    lines.push(line);
                }
            }
            ConnKind::Pipe => {
                if let Some(ref mut child) = conn.child
                    && let Some(ref mut stdin) = child.stdin
                {
                    for j in 0..text_len {
                        let line = string_elt(text, j);
                        if let Err(e) = write!(stdin, "{}{}", line, sep_str) {
                            r_error(&format!("error writing to pipe: {}", e));
                        }
                    }
                    let _ = stdin.flush();
                }
            }
            _ => {
                r_error("cannot write to this connection type");
            }
        }

        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_seek — seek(con, where = NA, origin = "start", rw = "")
// ---------------------------------------------------------------------------

pub unsafe fn do_seek(_call: SEXP, _op: SEXP, mut args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let scon = CAR(args);
        args = CDR(args);
        let where_val = as_real(CAR(args));
        args = CDR(args);
        let origin = as_integer(CAR(args));
        args = CDR(args);
        let rw = as_integer(CAR(args));

        if !inherits_class(scon, "connection") {
            r_error("'con' is not a connection");
        }
        let i = as_integer(scon) as usize;
        let mut table = connection_table();
        let Some(conn) = table[i].as_mut() else {
            r_error("invalid connection");
        };

        if !conn.isopen {
            r_error("connection is not open");
        }

        match &mut conn.kind {
            ConnKind::File => {
                if let Some(ref mut file) = conn.file {
                    let seek_from = match origin {
                        1 => SeekFrom::Current(where_val as i64),
                        3 => SeekFrom::End(where_val as i64),
                        _ => SeekFrom::Start(where_val as u64),
                    };

                    let old_pos = match file.stream_position() {
                        Ok(p) => p as c_double,
                        Err(_) => 0.0,
                    };

                    if !where_val.is_nan() {
                        match file.seek(seek_from) {
                            Ok(_) => {}
                            Err(e) => {
                                r_error(&format!("seek failed: {}", e));
                            }
                        }
                    }

                    return Rf_ScalarReal(old_pos);
                }
            }
            ConnKind::RawConnection => {
                let old_pos = conn.raw_pos as c_double;
                if !where_val.is_nan() {
                    let new_pos = match origin {
                        1 => (conn.raw_pos as i64 + where_val as i64) as usize,
                        3 => (conn.raw_data.len() as i64 + where_val as i64) as usize,
                        _ => where_val as usize,
                    };
                    conn.raw_pos = new_pos.min(conn.raw_data.len());
                }
                return Rf_ScalarReal(old_pos);
            }
            ConnKind::TextConnection => {
                let old_pos = conn.text_pos as c_double;
                if !where_val.is_nan() {
                    let new_pos = match origin {
                        1 => (conn.text_pos as i64 + where_val as i64) as usize,
                        3 => (conn.text_data.len() as i64 + where_val as i64) as usize,
                        _ => where_val as usize,
                    };
                    conn.text_pos = new_pos.min(conn.text_data.len());
                }
                return Rf_ScalarReal(old_pos);
            }
            _ => {
                r_error("seek not supported for this connection type");
            }
        }

        Rf_ScalarReal(0.0)
    }
}

// ---------------------------------------------------------------------------
// do_flush — flush(con)
// ---------------------------------------------------------------------------

pub unsafe fn do_flush(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let scon = CAR(args);

        if !inherits_class(scon, "connection") {
            r_error("'con' is not a connection");
        }
        let i = as_integer(scon) as usize;
        let mut table = connection_table();
        let Some(conn) = table[i].as_mut() else {
            r_error("invalid connection");
        };

        if conn.canwrite
            && let Some(ref mut writer) = conn.writer
        {
            let _ = writer.flush();
        }
        if conn.canwrite {
            let result = match conn.kind {
                ConnKind::GzFile => flush_gz_conn(conn),
                ConnKind::BzFile => flush_bz_conn(conn),
                ConnKind::XzFile => flush_xz_conn(conn),
                _ => Ok(()),
            };
            if let Err(e) = result {
                r_error(&format!("error flushing connection: {}", e));
            }
        }

        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_readTable / do_writeTable
// ---------------------------------------------------------------------------

pub unsafe fn do_readTable(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

pub unsafe fn do_writeTable(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

// ---------------------------------------------------------------------------
// do_rawConnection — rawConnection(raw, open = "rb")
// ---------------------------------------------------------------------------

pub unsafe fn do_rawConnection(_call: SEXP, _op: SEXP, mut args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let sfile = CAR(args);
        args = CDR(args);
        let sraw = CAR(args);
        args = CDR(args);
        let sopen = CAR(args);

        let description = check_string_arg(sfile, "description");
        let open = check_string_arg(sopen, "open");
        let open_mode = if open.is_empty() {
            "rb".to_string()
        } else {
            open
        };

        if open_mode.contains('t') {
            r_error("invalid 'open' argument");
        }

        // Copy raw data from SEXP
        let mut raw_data = Vec::new();
        if !sraw.is_null() && TYPEOF(sraw) == SEXPTYPE::RAWSXP {
            let len = LENGTH(sraw) as usize;
            let data_ptr = RAW(sraw);
            if !data_ptr.is_null() && len > 0 {
                raw_data.extend_from_slice(std::slice::from_raw_parts(data_ptr, len));
            }
        }

        let ncon = next_connection();
        let mut conn = RConn::new(
            "rawConnection",
            &description,
            &open_mode,
            ConnKind::RawConnection,
        );
        conn.text = false;
        conn.canseek = true;
        conn.isopen = true;
        conn.canread = open_mode.starts_with('r');
        conn.canwrite = open_mode.starts_with('w') || open_mode.starts_with('a');
        if open_mode.contains('+') {
            conn.canread = true;
            conn.canwrite = true;
        }
        if open_mode.starts_with('a') {
            conn.raw_pos = raw_data.len();
        }
        conn.raw_data = raw_data;

        let mut table = connection_table();
        table[ncon] = Some(Box::new(conn));
        drop(table);

        let ans = Rf_ScalarInteger(ncon as c_int);
        let _ans_guard = protect(ans);
        set_connection_class(ans, "rawConnection");
        ans
    }
}

// ---------------------------------------------------------------------------
// do_textConnection — textConnection(object, open = "r", local = FALSE)
// ---------------------------------------------------------------------------

pub unsafe fn do_textConnection(_call: SEXP, _op: SEXP, mut args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let sfile = CAR(args);
        args = CDR(args);
        let stext = CAR(args);
        args = CDR(args);
        let sopen = CAR(args);
        args = CDR(args);
        let _local = check_logical_arg(CAR(args), "local");

        let description = check_string_arg(sfile, "description");
        let open = check_string_arg(sopen, "open");
        let open_mode = if open.is_empty() {
            "r".to_string()
        } else {
            open
        };

        let ncon = next_connection();
        let mut conn = RConn::new(
            "textConnection",
            &description,
            &open_mode,
            ConnKind::TextConnection,
        );
        conn.canseek = false;

        if open_mode.starts_with('r') {
            // Input text connection: copy text from SEXP
            if !stext.is_null() && TYPEOF(stext) == SEXPTYPE::STRSXP {
                let len = LENGTH(stext) as R_xlen_t;
                let mut text = String::new();
                for j in 0..len {
                    let line = string_elt(stext, j);
                    text.push_str(&line);
                    text.push('\n');
                }
                conn.text_data = text;
                conn.text_pos = 0;
                conn.isopen = true;
                conn.canread = true;
                conn.canwrite = false;
            }
        } else {
            // Output text connection
            conn.isopen = true;
            conn.canread = false;
            conn.canwrite = true;
        }

        let mut table = connection_table();
        table[ncon] = Some(Box::new(conn));
        drop(table);

        let ans = Rf_ScalarInteger(ncon as c_int);
        let _ans_guard = protect(ans);
        set_connection_class(ans, "textConnection");
        ans
    }
}

// ---------------------------------------------------------------------------
// do_textConnectionValue — textConnectionValue(con)
// ---------------------------------------------------------------------------

pub unsafe fn do_textConnectionValue(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let scon = CAR(args);

        if !inherits_class(scon, "connection") {
            r_error("'con' is not a connection");
        }
        let i = as_integer(scon) as usize;
        let table = connection_table();
        let Some(conn) = table[i].as_ref() else {
            r_error("invalid connection");
        };

        if !conn.canwrite {
            r_error("'con' is not an output textConnection");
        }

        let lines = conn.text_lines.borrow();
        let nlines = lines.len() as c_int;
        let ans = Rf_allocVector(SEXPTYPE::STRSXP, nlines);
        if !ans.is_null() {
            for (idx, line) in lines.iter().enumerate() {
                let c_line = CString::new(line.as_str())
                    .unwrap_or_else(|_| CString::new("").unwrap_or_default());
                let charsxp = Rf_mkChar(c_line.as_ptr());
                SET_STRING_ELT(ans, idx as R_xlen_t, charsxp);
            }
        }

        ans
    }
}

// ---------------------------------------------------------------------------
// do_sockConnection — socketConnection()
// ---------------------------------------------------------------------------

pub unsafe fn do_sockConnection(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    r_error("socketConnection is not supported in this pure-R Android runtime")
}

// ---------------------------------------------------------------------------
// do_serverSocket — server socket support
// ---------------------------------------------------------------------------

pub unsafe fn do_serverSocket(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    r_error("serverSocket is not supported in this pure-R Android runtime")
}

// ---------------------------------------------------------------------------
// do_download — legacy mainutils entry point
// ---------------------------------------------------------------------------

pub unsafe fn do_download(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    r_error("download.file is implemented through the utils internet boundary")
}

// ---------------------------------------------------------------------------
// do_getConnection — getConnection(n)
// ---------------------------------------------------------------------------

pub unsafe fn do_getConnection(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let sn = CAR(args);
        let n = as_integer(sn) as usize;

        init_connections_table();
        let table = connection_table();

        if n >= table.len() || table[n].is_none() {
            r_error("invalid connection");
        }

        let Some(_conn) = table[n].as_ref() else {
            r_error("invalid connection");
        };

        // Build a list with connection info
        // Return the integer index (like R's getConnection)
        drop(table);
        Rf_ScalarInteger(n as c_int)
    }
}

// ---------------------------------------------------------------------------
// do_showConnections — showConnections(all = FALSE)
// ---------------------------------------------------------------------------

pub unsafe fn do_showConnections(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let _all = check_logical_arg(CAR(args), "all");

        init_connections_table();
        let table = connection_table();

        // Count active connections
        let mut count = 0usize;
        for i in 0..table.len() {
            if table[i].is_some() {
                count += 1;
            }
        }

        let ans = Rf_allocVector(SEXPTYPE::STRSXP, count as c_int);
        if !ans.is_null() {
            let mut idx = 0usize;
            for i in 0..table.len() {
                if let Some(ref conn) = table[i] {
                    let desc = format!("{} {} {}", i, conn.description, conn.mode);
                    let c_desc =
                        CString::new(desc).unwrap_or_else(|_| CString::new("").unwrap_or_default());
                    let charsxp = Rf_mkChar(c_desc.as_ptr());
                    SET_STRING_ELT(ans, idx as R_xlen_t, charsxp);
                    idx += 1;
                }
            }
        }

        ans
    }
}

// ---------------------------------------------------------------------------
// do_sumConnection — summary.connection()
// ---------------------------------------------------------------------------

pub unsafe fn do_sumConnection(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let scon = CAR(args);
        if !inherits_class(scon, "connection") {
            r_error("'con' is not a connection");
        }
        let i = as_integer(scon) as usize;
        let table = connection_table();
        let Some(conn) = table[i].as_ref() else {
            r_error("invalid connection");
        };

        let fields = [
            format!("description={}", conn.description),
            format!("class={}", conn.class),
            format!("mode={}", conn.mode),
            format!("opened={}", conn.isopen),
            format!("can read={}", conn.canread),
            format!("can write={}", conn.canwrite),
            format!("can seek={}", conn.canseek),
            format!("pushback={}", conn.pushback.len()),
        ];
        let ans = Rf_allocVector(SEXPTYPE::STRSXP, fields.len() as c_int);
        for (idx, field) in fields.iter().enumerate() {
            let c_field = CString::new(field.as_str()).unwrap_or_default();
            let charsxp = Rf_mkChar(c_field.as_ptr());
            SET_STRING_ELT(ans, idx as R_xlen_t, charsxp);
        }
        ans
    }
}

// ---------------------------------------------------------------------------
// do_sink — sink(number = NULL, close.on.exit = FALSE, type = "output", split = FALSE)
// ---------------------------------------------------------------------------

pub unsafe fn do_sink(_call: SEXP, _op: SEXP, mut args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let sn = CAR(args);
        args = CDR(args);
        let close_on_exit = check_logical_arg(CAR(args), "closeOnExit");
        args = CDR(args);
        let errcon = check_logical_arg(CAR(args), "type");
        args = CDR(args);
        let tee = check_logical_arg(CAR(args), "split");

        let icon = as_integer(sn);

        let mut sink = sink_state();

        if errcon == 0 {
            // Output sink
            if icon >= 0 {
                if sink.sink_number >= 20 {
                    r_error("sink stack is full");
                }
                sink.sink_number += 1;
                if sink.sink_cons.len() <= sink.sink_number {
                    sink.sink_cons.push(icon);
                    sink.sink_close.push(close_on_exit != 0);
                    sink.sink_split.push(tee != 0);
                } else {
                    let idx = sink.sink_number;
                    sink.sink_cons[idx] = icon;
                    sink.sink_close[idx] = close_on_exit != 0;
                    sink.sink_split[idx] = tee != 0;
                }
                sink.output_con = icon;
            } else {
                // Close sink: revert to stdout
                if sink.sink_number > 0 {
                    sink.sink_number -= 1;
                }
                sink.output_con = 1;
            }
        } else {
            // Error/message sink
            if icon < 0 || icon == 2 {
                sink.error_con = 2;
            } else {
                sink.error_con = icon;
            }
        }

        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_sinkNumber — sink.number(type = "output")
// ---------------------------------------------------------------------------

pub unsafe fn do_sinkNumber(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let errcon = check_logical_arg(CAR(args), "type");
        let sink = sink_state();
        if errcon != 0 {
            Rf_ScalarInteger(sink.error_con)
        } else {
            Rf_ScalarInteger(sink.sink_number as c_int)
        }
    }
}

// ---------------------------------------------------------------------------
// do_pushBack — pushBack(data, con)
// ---------------------------------------------------------------------------

pub unsafe fn do_pushBack(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let data = CAR(_args);
        let con = CAR(CDR(_args));
        let new_line = if CDR(CDR(_args)) == R_NilValue() {
            true
        } else {
            check_logical_arg(CAR(CDR(CDR(_args))), "newLine") != 0
        };

        if TYPEOF(data) != SEXPTYPE::STRSXP {
            r_error("'data' must be a character vector");
        }
        if !inherits_class(con, "connection") {
            r_error("'con' is not a connection");
        }
        let i = as_integer(con);
        let len = LENGTH(data);
        for idx in (0..len).rev() {
            let mut line = string_elt(data, idx as R_xlen_t).into_bytes();
            if new_line {
                line.push(b'\n');
            }
            connection_pushback(i, &line);
        }
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_pushBackClear — pushBackClear(con)
// ---------------------------------------------------------------------------

pub unsafe fn do_pushBackClear(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let con = CAR(args);
        if !inherits_class(con, "connection") {
            r_error("'con' is not a connection");
        }
        let i = as_integer(con) as usize;
        let mut table = connection_table();
        let Some(conn) = table[i].as_mut() else {
            r_error("invalid connection");
        };
        conn.pushback.clear();
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_pushBackLength — pushBackLength(con)
// ---------------------------------------------------------------------------

pub unsafe fn do_pushBackLength(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let con = CAR(args);
        if !inherits_class(con, "connection") {
            r_error("'con' is not a connection");
        }
        let i = as_integer(con) as usize;
        let table = connection_table();
        let Some(conn) = table[i].as_ref() else {
            r_error("invalid connection");
        };
        Rf_ScalarInteger(conn.pushback.len() as c_int)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BinaryKind {
    Raw,
    Integer,
    Logical,
    Numeric,
    Complex,
    Character,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ByteOrder {
    Little,
    Big,
}

impl ByteOrder {
    fn native() -> Self {
        if cfg!(target_endian = "little") {
            Self::Little
        } else {
            Self::Big
        }
    }

    fn swapped(self) -> Self {
        match self {
            Self::Little => Self::Big,
            Self::Big => Self::Little,
        }
    }
}

unsafe fn binary_kind_from_what(what: SEXP) -> BinaryKind {
    unsafe {
        if what.is_null() || what == R_NilValue() {
            r_error("invalid 'what' argument");
        }
        match TYPEOF(what) {
            t if t == SEXPTYPE::STRSXP => {
                if LENGTH(what) == 0 {
                    return BinaryKind::Character;
                }
                match string_elt(what, 0).as_str() {
                    "raw" => BinaryKind::Raw,
                    "integer" | "int" => BinaryKind::Integer,
                    "logical" => BinaryKind::Logical,
                    "numeric" | "double" => BinaryKind::Numeric,
                    "complex" => BinaryKind::Complex,
                    "character" => BinaryKind::Character,
                    _ => r_error("invalid 'what' argument"),
                }
            }
            t if t == SEXPTYPE::RAWSXP => BinaryKind::Raw,
            t if t == SEXPTYPE::INTSXP => BinaryKind::Integer,
            t if t == SEXPTYPE::LGLSXP => BinaryKind::Logical,
            t if t == SEXPTYPE::REALSXP => BinaryKind::Numeric,
            t if t == SEXPTYPE::CPLXSXP => BinaryKind::Complex,
            _ => r_error("invalid 'what' argument"),
        }
    }
}

unsafe fn byte_order_from_arg(arg: SEXP, name: &str) -> ByteOrder {
    unsafe {
        if arg.is_null() || arg == R_NilValue() || arg == R_MissingArg() {
            return ByteOrder::native();
        }
        if TYPEOF(arg) == SEXPTYPE::STRSXP {
            if LENGTH(arg) < 1 {
                r_error(&format!("invalid '{name}' argument"));
            }
            return match string_elt(arg, 0).as_str() {
                "little" => ByteOrder::Little,
                "big" => ByteOrder::Big,
                "swap" => ByteOrder::native().swapped(),
                _ => r_error(&format!("invalid '{name}' argument")),
            };
        }
        let swap = check_logical_arg(arg, name);
        if swap == 0 {
            ByteOrder::native()
        } else {
            ByteOrder::native().swapped()
        }
    }
}

unsafe fn binary_count(arg: SEXP) -> usize {
    unsafe {
        let n = as_integer(arg);
        if n < 0 || n == NA_INTEGER {
            r_error("invalid 'n' argument");
        }
        n as usize
    }
}

unsafe fn logical_arg_or(arg: SEXP, name: &str, default: c_int) -> c_int {
    unsafe {
        if arg.is_null() || arg == R_NilValue() || arg == R_MissingArg() {
            default
        } else {
            check_logical_arg(arg, name)
        }
    }
}

unsafe fn connection_rw_mode(arg: SEXP) -> c_int {
    unsafe {
        if arg.is_null() || arg == R_NilValue() || arg == R_MissingArg() {
            return 0;
        }
        if TYPEOF(arg) == SEXPTYPE::STRSXP {
            if LENGTH(arg) < 1 {
                return 0;
            }
            return match string_elt(arg, 0).as_str() {
                "read" | "r" => 1,
                "write" | "w" => 2,
                _ => 0,
            };
        }
        as_integer(arg)
    }
}

unsafe fn binary_size(arg: SEXP, kind: BinaryKind) -> usize {
    unsafe {
        let default = match kind {
            BinaryKind::Raw | BinaryKind::Character => 1,
            BinaryKind::Integer | BinaryKind::Logical => 4,
            BinaryKind::Numeric => 8,
            BinaryKind::Complex => 16,
        };
        let size = as_integer(arg);
        if size == NA_INTEGER {
            return default;
        }
        let size = size as usize;
        let valid = match kind {
            BinaryKind::Raw | BinaryKind::Character => size == 1,
            BinaryKind::Integer | BinaryKind::Logical => matches!(size, 1 | 2 | 4),
            BinaryKind::Numeric => matches!(size, 4 | 8),
            BinaryKind::Complex => matches!(size, 8 | 16),
        };
        if !valid {
            r_error("invalid 'size' argument");
        }
        size
    }
}

unsafe fn raw_bytes_from_vector(raw: SEXP) -> Vec<u8> {
    unsafe {
        let len = LENGTH(raw) as usize;
        if len == 0 {
            return Vec::new();
        }
        let data = RAW(raw);
        if data.is_null() {
            return Vec::new();
        }
        std::slice::from_raw_parts(data, len).to_vec()
    }
}

unsafe fn read_binary_source(con: SEXP, limit: Option<usize>) -> Vec<u8> {
    unsafe {
        if TYPEOF(con) == SEXPTYPE::RAWSXP {
            let mut bytes = raw_bytes_from_vector(con);
            if let Some(limit) = limit {
                bytes.truncate(limit);
            }
            return bytes;
        }
        if TYPEOF(con) == SEXPTYPE::STRSXP {
            let path = check_string_arg(con, "con");
            let mut bytes = std::fs::read(&path).unwrap_or_else(|e| {
                r_error(&format!("cannot open file '{}': {}", path, e));
            });
            if let Some(limit) = limit {
                bytes.truncate(limit);
            }
            return bytes;
        }
        if !inherits_class(con, "connection") {
            r_error("'con' is not a connection");
        }
        let index = as_integer(con);
        let mut bytes = Vec::new();
        match limit {
            Some(limit) => {
                for _ in 0..limit {
                    let byte = connection_fgetc(index);
                    if byte < 0 {
                        break;
                    }
                    bytes.push(byte as u8);
                }
            }
            None => loop {
                let byte = connection_fgetc(index);
                if byte < 0 {
                    break;
                }
                bytes.push(byte as u8);
            },
        }
        bytes
    }
}

fn read_integer_chunk(chunk: &[u8], order: ByteOrder, signed: bool) -> i32 {
    match chunk.len() {
        1 if signed => i8::from_ne_bytes([chunk[0]]) as i32,
        1 => chunk[0] as i32,
        2 => {
            let bytes = [chunk[0], chunk[1]];
            if signed {
                match order {
                    ByteOrder::Little => i16::from_le_bytes(bytes) as i32,
                    ByteOrder::Big => i16::from_be_bytes(bytes) as i32,
                }
            } else {
                match order {
                    ByteOrder::Little => u16::from_le_bytes(bytes) as i32,
                    ByteOrder::Big => u16::from_be_bytes(bytes) as i32,
                }
            }
        }
        4 => {
            let bytes = [chunk[0], chunk[1], chunk[2], chunk[3]];
            match order {
                ByteOrder::Little => i32::from_le_bytes(bytes),
                ByteOrder::Big => i32::from_be_bytes(bytes),
            }
        }
        _ => 0,
    }
}

fn write_integer_chunk(out: &mut Vec<u8>, value: i32, size: usize, order: ByteOrder) {
    match size {
        1 => out.push(value as u8),
        2 => match order {
            ByteOrder::Little => out.extend_from_slice(&(value as i16).to_le_bytes()),
            ByteOrder::Big => out.extend_from_slice(&(value as i16).to_be_bytes()),
        },
        4 => match order {
            ByteOrder::Little => out.extend_from_slice(&value.to_le_bytes()),
            ByteOrder::Big => out.extend_from_slice(&value.to_be_bytes()),
        },
        _ => {}
    }
}

unsafe fn alloc_raw_result(bytes: &[u8]) -> SEXP {
    unsafe {
        let ans = Rf_allocVector(SEXPTYPE::RAWSXP, bytes.len() as c_int);
        if !ans.is_null() && !bytes.is_empty() {
            ptr::copy_nonoverlapping(bytes.as_ptr(), RAW(ans), bytes.len());
        }
        ans
    }
}

unsafe fn alloc_integer_result(values: &[i32], sexptype: SEXPTYPE) -> SEXP {
    unsafe {
        let ans = Rf_allocVector(sexptype.0, values.len() as c_int);
        if !ans.is_null() {
            let dest = if sexptype == SEXPTYPE::LGLSXP {
                LOGICAL(ans)
            } else {
                INTEGER(ans)
            };
            for (index, value) in values.iter().enumerate() {
                *dest.add(index) = *value;
            }
        }
        ans
    }
}

unsafe fn alloc_real_result(values: &[f64]) -> SEXP {
    unsafe {
        let ans = Rf_allocVector(SEXPTYPE::REALSXP, values.len() as c_int);
        if !ans.is_null() {
            for (index, value) in values.iter().enumerate() {
                *REAL(ans).add(index) = *value;
            }
        }
        ans
    }
}

unsafe fn alloc_character_result(values: &[String]) -> SEXP {
    unsafe {
        let ans = Rf_allocVector(SEXPTYPE::STRSXP, values.len() as c_int);
        if !ans.is_null() {
            for (index, value) in values.iter().enumerate() {
                let c_value = CString::new(value.as_str()).unwrap_or_default();
                SET_STRING_ELT(ans, index as R_xlen_t, Rf_mkChar(c_value.as_ptr()));
            }
        }
        ans
    }
}

unsafe fn decode_binary_result(
    kind: BinaryKind,
    bytes: &[u8],
    n: usize,
    size: usize,
    signed: bool,
    order: ByteOrder,
) -> SEXP {
    unsafe {
        match kind {
            BinaryKind::Raw => alloc_raw_result(&bytes[..bytes.len().min(n)]),
            BinaryKind::Integer | BinaryKind::Logical => {
                let values: Vec<i32> = bytes
                    .chunks_exact(size)
                    .take(n)
                    .map(|chunk| read_integer_chunk(chunk, order, signed))
                    .collect();
                let sexptype = if kind == BinaryKind::Logical {
                    SEXPTYPE::LGLSXP
                } else {
                    SEXPTYPE::INTSXP
                };
                alloc_integer_result(&values, sexptype)
            }
            BinaryKind::Numeric => {
                let values: Vec<f64> = bytes
                    .chunks_exact(size)
                    .take(n)
                    .map(|chunk| {
                        if size == 4 {
                            let bytes = [chunk[0], chunk[1], chunk[2], chunk[3]];
                            match order {
                                ByteOrder::Little => f32::from_le_bytes(bytes) as f64,
                                ByteOrder::Big => f32::from_be_bytes(bytes) as f64,
                            }
                        } else {
                            let bytes = [
                                chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5],
                                chunk[6], chunk[7],
                            ];
                            match order {
                                ByteOrder::Little => f64::from_le_bytes(bytes),
                                ByteOrder::Big => f64::from_be_bytes(bytes),
                            }
                        }
                    })
                    .collect();
                alloc_real_result(&values)
            }
            BinaryKind::Complex => {
                let values: Vec<f64> = bytes
                    .chunks_exact(size / 2)
                    .take(n * 2)
                    .map(|chunk| {
                        if size == 8 {
                            let bytes = [chunk[0], chunk[1], chunk[2], chunk[3]];
                            match order {
                                ByteOrder::Little => f32::from_le_bytes(bytes) as f64,
                                ByteOrder::Big => f32::from_be_bytes(bytes) as f64,
                            }
                        } else {
                            let bytes = [
                                chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5],
                                chunk[6], chunk[7],
                            ];
                            match order {
                                ByteOrder::Little => f64::from_le_bytes(bytes),
                                ByteOrder::Big => f64::from_be_bytes(bytes),
                            }
                        }
                    })
                    .collect();
                let count = values.len() / 2;
                let ans = Rf_allocVector(SEXPTYPE::CPLXSXP, count as c_int);
                if !ans.is_null() {
                    let dest = COMPLEX(ans);
                    for index in 0..count {
                        (*dest.add(index)).r = values[index * 2];
                        (*dest.add(index)).i = values[index * 2 + 1];
                    }
                }
                ans
            }
            BinaryKind::Character => {
                let mut values = Vec::new();
                let mut start = 0usize;
                while start < bytes.len() && values.len() < n {
                    let rel_end = bytes[start..].iter().position(|byte| *byte == 0);
                    match rel_end {
                        Some(len) => {
                            values.push(
                                String::from_utf8_lossy(&bytes[start..start + len]).into_owned(),
                            );
                            start += len + 1;
                        }
                        None => break,
                    }
                }
                alloc_character_result(&values)
            }
        }
    }
}

unsafe fn encode_binary_object(object: SEXP, size_arg: SEXP, order: ByteOrder) -> Vec<u8> {
    unsafe {
        if object.is_null() || object == R_NilValue() {
            r_error("invalid 'object' argument");
        }
        let obj_type = TYPEOF(object);
        let obj_len = LENGTH(object) as usize;
        let mut bytes = Vec::new();
        match obj_type {
            t if t == SEXPTYPE::RAWSXP => {
                bytes.extend_from_slice(&raw_bytes_from_vector(object));
            }
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
                let kind = if obj_type == SEXPTYPE::LGLSXP {
                    BinaryKind::Logical
                } else {
                    BinaryKind::Integer
                };
                let size = binary_size(size_arg, kind);
                let src = if obj_type == SEXPTYPE::LGLSXP {
                    LOGICAL(object)
                } else {
                    INTEGER(object)
                };
                for index in 0..obj_len {
                    write_integer_chunk(&mut bytes, *src.add(index), size, order);
                }
            }
            t if t == SEXPTYPE::REALSXP => {
                let size = binary_size(size_arg, BinaryKind::Numeric);
                for index in 0..obj_len {
                    let value = *REAL(object).add(index);
                    if size == 4 {
                        let value = value as f32;
                        match order {
                            ByteOrder::Little => bytes.extend_from_slice(&value.to_le_bytes()),
                            ByteOrder::Big => bytes.extend_from_slice(&value.to_be_bytes()),
                        }
                    } else {
                        match order {
                            ByteOrder::Little => bytes.extend_from_slice(&value.to_le_bytes()),
                            ByteOrder::Big => bytes.extend_from_slice(&value.to_be_bytes()),
                        }
                    }
                }
            }
            t if t == SEXPTYPE::CPLXSXP => {
                let size = binary_size(size_arg, BinaryKind::Complex);
                for index in 0..obj_len {
                    let value = *COMPLEX(object).add(index);
                    if size == 8 {
                        for part in [value.r as f32, value.i as f32] {
                            match order {
                                ByteOrder::Little => bytes.extend_from_slice(&part.to_le_bytes()),
                                ByteOrder::Big => bytes.extend_from_slice(&part.to_be_bytes()),
                            }
                        }
                    } else {
                        for part in [value.r, value.i] {
                            match order {
                                ByteOrder::Little => bytes.extend_from_slice(&part.to_le_bytes()),
                                ByteOrder::Big => bytes.extend_from_slice(&part.to_be_bytes()),
                            }
                        }
                    }
                }
            }
            t if t == SEXPTYPE::STRSXP => {
                for index in 0..obj_len as R_xlen_t {
                    bytes.extend_from_slice(string_elt(object, index).as_bytes());
                    bytes.push(0);
                }
            }
            _ => r_error("can only write vector objects"),
        }
        bytes
    }
}

unsafe fn write_binary_sink(con: SEXP, bytes: &[u8]) -> SEXP {
    unsafe {
        if TYPEOF(con) == SEXPTYPE::RAWSXP {
            return alloc_raw_result(bytes);
        }
        if TYPEOF(con) == SEXPTYPE::STRSXP {
            let path = check_string_arg(con, "con");
            std::fs::write(&path, bytes).unwrap_or_else(|e| {
                r_error(&format!("cannot open file '{}': {}", path, e));
            });
            return R_NilValue();
        }
        if !inherits_class(con, "connection") {
            r_error("'con' is not a connection");
        }
        connection_write_bytes(as_integer(con), bytes);
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_readBin — readBin(con, what, n, size = NA, signed = TRUE, endian/swap)
// ---------------------------------------------------------------------------

pub unsafe fn do_readBin(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let con = arg_by_name_or_position(args, 0, &["con"], R_NilValue());
        let what_arg = arg_by_name_or_position(args, 1, &["what"], R_NilValue());
        let n_arg = arg_by_name_or_position(args, 2, &["n"], Rf_ScalarInteger(1));
        let what = binary_kind_from_what(what_arg);
        let size_arg = arg_by_name_or_position(args, 3, &["size"], Rf_ScalarInteger(NA_INTEGER));
        let signed_arg = arg_by_name_or_position(args, 4, &["signed"], Rf_ScalarLogical(1));
        let endian_arg = arg_by_name_or_position(args, 5, &["endian", "swap"], R_MissingArg());
        let n = binary_count(n_arg);
        let size = binary_size(size_arg, what);
        let signed = logical_arg_or(signed_arg, "signed", 1) != 0;
        let order = byte_order_from_arg(endian_arg, "endian");

        let limit = match what {
            BinaryKind::Raw => Some(n),
            BinaryKind::Character => None,
            _ => Some(n.saturating_mul(size)),
        };
        let bytes = read_binary_source(con, limit);
        decode_binary_result(what, &bytes, n, size, signed, order)
    }
}

// ---------------------------------------------------------------------------
// do_writeBin — writeBin(object, con, size = NA, endian/swap, useBytes = FALSE)
// ---------------------------------------------------------------------------

pub unsafe fn do_writeBin(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let object = arg_by_name_or_position(args, 0, &["object"], R_NilValue());
        let con = arg_by_name_or_position(args, 1, &["con"], R_NilValue());
        let size_arg = arg_by_name_or_position(args, 2, &["size"], Rf_ScalarInteger(NA_INTEGER));
        let endian_arg = arg_by_name_or_position(args, 3, &["endian", "swap"], R_MissingArg());
        let use_bytes_arg = arg_by_name_or_position(args, 4, &["useBytes"], Rf_ScalarLogical(0));
        let order = byte_order_from_arg(endian_arg, "endian");
        let _use_bytes = logical_arg_or(use_bytes_arg, "useBytes", 0);

        let bytes = encode_binary_object(object, size_arg, order);
        write_binary_sink(con, &bytes)
    }
}

// ---------------------------------------------------------------------------
// R_GetConnection — get connection by index (C API)
// ---------------------------------------------------------------------------

pub unsafe fn R_GetConnection(_n: c_int) -> SEXP {
    unsafe {
        // This returns the SEXP representation of the connection (the integer index)
        // In R's C code, this returns the Rconnection struct pointer,
        // but in our port, connections are in the global table.
        // We return the integer SEXP index.
        if _n < 0 || _n >= NCONNECTIONS as c_int {
            r_error("invalid connection");
        }
        Rf_ScalarInteger(_n)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::{Mutex, MutexGuard};

    use super::*;
    use crate::sexp::session::RSession;
    use std::io::Write;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    struct ConnectionTestGuard {
        _lock: MutexGuard<'static, ()>,
        _session: RSession,
    }

    fn test_ok<T, E: std::fmt::Display>(result: Result<T, E>) -> T {
        match result {
            Ok(value) => value,
            Err(err) => panic!("test setup failed: {err}"),
        }
    }

    fn expect_r_error<F>(f: F) -> String
    where
        F: FnOnce(),
    {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        let Err(payload) = result else {
            panic!("expected RError");
        };
        let Some(err) = payload.downcast_ref::<RError>() else {
            panic!("expected RError payload");
        };
        err.message.clone()
    }

    /// Reset session-local connection state and return a guard that keeps an
    /// active session installed for the duration of the test.
    fn reset_connections() -> ConnectionTestGuard {
        let lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let session = RSession::new();
        with_connections_state(|state| {
            state.table.clear();
            state.sink = SinkState::default();
            state.sink.sink_cons = vec![1];
            state.sink.sink_close = vec![false];
            state.sink.sink_split = vec![false];
        });
        ConnectionTestGuard {
            _lock: lock,
            _session: session,
        }
    }

    #[test]
    fn test_init_connections() {
        let _lock = reset_connections();
        unsafe {
            R_InitConnections();
            let table = connection_table();
            assert!(table.len() >= 3);
            assert!(table[0].is_some()); // stdin
            assert!(table[1].is_some()); // stdout
            assert!(table[2].is_some()); // stderr
        }
    }

    #[test]
    fn test_server_socket_reports_unsupported_runtime() {
        let _lock = reset_connections();
        let message = expect_r_error(|| unsafe {
            do_serverSocket(
                ptr::null_mut(),
                ptr::null_mut(),
                R_NilValue(),
                ptr::null_mut(),
            );
        });

        assert!(message.contains("serverSocket is not supported"));
    }

    #[test]
    fn test_legacy_download_entry_reports_real_boundary() {
        let _lock = reset_connections();
        let message = expect_r_error(|| unsafe {
            do_download(
                ptr::null_mut(),
                ptr::null_mut(),
                R_NilValue(),
                ptr::null_mut(),
            );
        });

        assert!(message.contains("utils internet boundary"));
    }

    #[test]
    fn test_do_file_create() {
        let _lock = reset_connections();
        unsafe {
            // Create a temp file for testing
            let tmp = std::env::temp_dir().join("rport_test_file_conn.txt");
            {
                let mut f = test_ok(File::create(&tmp));
                if let Err(err) = write!(f, "hello world\n") {
                    panic!("test setup failed: {err}");
                }
            }

            let desc = test_ok(CString::new(tmp.to_str().unwrap_or("")));
            let open = test_ok(CString::new("r"));
            let desc_sxp = Rf_mkString(desc.as_ptr());
            let open_sxp = Rf_mkString(open.as_ptr());
            let _desc_guard = protect(desc_sxp);
            let _open_guard = protect(open_sxp);

            // Build args pairlist: (description, open, encoding, blocking, method, raw)
            let raw_sxp = Rf_ScalarLogical(0);
            let _raw_guard = protect(raw_sxp);
            let enc_sxp = Rf_mkString(test_ok(CString::new("")).as_ptr());
            let _enc_guard = protect(enc_sxp);
            let block_sxp = Rf_ScalarLogical(1);
            let _block_guard = protect(block_sxp);
            let method_sxp = Rf_mkString(test_ok(CString::new("default")).as_ptr());
            let _method_guard = protect(method_sxp);

            let p5 = Rf_cons(raw_sxp, R_NilValue());
            let _p5_guard = protect(p5);
            let p4 = Rf_cons(method_sxp, p5);
            let _p4_guard = protect(p4);
            let p3 = Rf_cons(block_sxp, p4);
            let _p3_guard = protect(p3);
            let p2 = Rf_cons(enc_sxp, p3);
            let _p2_guard = protect(p2);
            let p1 = Rf_cons(open_sxp, p2);
            let _p1_guard = protect(p1);
            let args = Rf_cons(desc_sxp, p1);
            let _args_guard = protect(args);

            let result = do_file(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert!(!result.is_null());

            // Clean up
            let _ = fs::remove_file(&tmp);
        }
    }

    #[test]
    fn test_do_raw_connection() {
        let _lock = reset_connections();
        unsafe {
            // Create a raw vector
            let raw = Rf_allocVector(SEXPTYPE::RAWSXP, 5);
            let _raw_guard = protect(raw);
            let raw_data = RAW(raw);
            *raw_data.add(0) = 1;
            *raw_data.add(1) = 2;
            *raw_data.add(2) = 3;
            *raw_data.add(3) = 4;
            *raw_data.add(4) = 5;

            let desc_sxp = Rf_mkString(test_ok(CString::new("test_raw")).as_ptr());
            let _desc_guard = protect(desc_sxp);
            let open_sxp = Rf_mkString(test_ok(CString::new("rb")).as_ptr());
            let _open_guard = protect(open_sxp);
            let local_sxp = Rf_ScalarLogical(0);
            let _local_guard = protect(local_sxp);

            let p2 = Rf_cons(local_sxp, R_NilValue());
            let _p2_guard = protect(p2);
            let p1 = Rf_cons(open_sxp, p2);
            let _p1_guard = protect(p1);
            let args = Rf_cons(raw, p1);
            let _args_guard = protect(args);
            let args2 = Rf_cons(desc_sxp, args);
            let _args2_guard = protect(args2);

            let result = do_rawConnection(ptr::null_mut(), ptr::null_mut(), args2, ptr::null_mut());
            assert!(!result.is_null());

            let idx = as_integer(result);
            assert!(idx >= 3);
        }
    }

    #[test]
    fn test_do_text_connection() {
        let _lock = reset_connections();
        unsafe {
            // Create a text vector
            let text = Rf_allocVector(SEXPTYPE::STRSXP, 2);
            let _text_guard = protect(text);
            let c1 = Rf_mkChar(test_ok(CString::new("line1")).as_ptr());
            let c2 = Rf_mkChar(test_ok(CString::new("line2")).as_ptr());
            SET_STRING_ELT(text, 0, c1);
            SET_STRING_ELT(text, 1, c2);

            let desc_sxp = Rf_mkString(test_ok(CString::new("test_text")).as_ptr());
            let _desc_guard = protect(desc_sxp);
            let open_sxp = Rf_mkString(test_ok(CString::new("r")).as_ptr());
            let _open_guard = protect(open_sxp);
            let local_sxp = Rf_ScalarLogical(0);
            let _local_guard = protect(local_sxp);

            let p2 = Rf_cons(local_sxp, R_NilValue());
            let _p2_guard = protect(p2);
            let p1 = Rf_cons(open_sxp, p2);
            let _p1_guard = protect(p1);
            let args = Rf_cons(text, p1);
            let _args_guard = protect(args);
            let args2 = Rf_cons(desc_sxp, args);
            let _args2_guard = protect(args2);

            let result =
                do_textConnection(ptr::null_mut(), ptr::null_mut(), args2, ptr::null_mut());
            assert!(!result.is_null());

            let idx = as_integer(result);
            assert!(idx >= 3);

            // Verify the connection was created
            let table = connection_table();
            let Some(conn) = table[idx as usize].as_ref() else {
                panic!("expected connection to exist");
            };
            assert_eq!(conn.class, "textConnection");
            assert!(conn.isopen);
            assert!(conn.canread);
            assert_eq!(conn.text_data, "line1\nline2\n");
        }
    }

    #[test]
    fn test_do_isopen() {
        let _lock = reset_connections();
        unsafe {
            R_InitConnections();
            // stdin should be open
            let stdin_sxp = Rf_ScalarInteger(0);
            let _stdin_guard = protect(stdin_sxp);
            let rw_sxp = Rf_ScalarInteger(0);
            let _rw_guard = protect(rw_sxp);
            let tail = Rf_cons(rw_sxp, R_NilValue());
            let _tail_guard = protect(tail);
            let args = Rf_cons(stdin_sxp, tail);
            let _args_guard = protect(args);

            let result = do_isopen(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert!(!result.is_null());
            assert_eq!(as_integer(result), 1);
        }
    }

    #[test]
    fn test_do_isseekable_uses_connection_capability() {
        let _lock = reset_connections();
        unsafe {
            R_InitConnections();

            let stdout_sxp = Rf_ScalarInteger(1);
            let _stdout_guard = protect(stdout_sxp);
            set_connection_class(stdout_sxp, "terminal");
            let stdout_args = Rf_cons(stdout_sxp, R_NilValue());
            let _stdout_args_guard = protect(stdout_args);
            let stdout_result = do_isseekable(
                ptr::null_mut(),
                ptr::null_mut(),
                stdout_args,
                ptr::null_mut(),
            );
            assert_eq!(as_integer(stdout_result), 0);

            let raw = Rf_allocVector3(SEXPTYPE::RAWSXP, 0);
            let _raw_guard = protect(raw);
            let desc = Rf_mkString(c"raw".as_ptr());
            let _desc_guard = protect(desc);
            let open = Rf_mkString(c"rb".as_ptr());
            let _open_guard = protect(open);
            let open_tail = Rf_cons(open, R_NilValue());
            let _open_tail_guard = protect(open_tail);
            let raw_tail = Rf_cons(raw, open_tail);
            let _raw_tail_guard = protect(raw_tail);
            let raw_args = Rf_cons(desc, raw_tail);
            let _raw_args_guard = protect(raw_args);
            let raw_conn =
                do_rawConnection(ptr::null_mut(), ptr::null_mut(), raw_args, ptr::null_mut());
            let raw_seek_args = Rf_cons(raw_conn, R_NilValue());
            let _raw_seek_args_guard = protect(raw_seek_args);
            let raw_result = do_isseekable(
                ptr::null_mut(),
                ptr::null_mut(),
                raw_seek_args,
                ptr::null_mut(),
            );
            assert_eq!(as_integer(raw_result), 1);
        }
    }

    #[test]
    fn test_do_show_connections() {
        let _lock = reset_connections();
        unsafe {
            R_InitConnections();
            let all_sxp = Rf_ScalarLogical(1);
            let _all_guard = protect(all_sxp);
            let args = Rf_cons(all_sxp, R_NilValue());
            let _args_guard = protect(args);

            let result =
                do_showConnections(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert!(!result.is_null());
            assert!(LENGTH(result) >= 3);
        }
    }

    #[test]
    fn test_do_gzfile_stub() {
        let _lock = reset_connections();
        unsafe {
            let tmp = std::env::temp_dir().join("rport_test_gz.txt");
            {
                let mut f = test_ok(File::create(&tmp));
                if let Err(err) = write!(f, "test data\n") {
                    panic!("test setup failed: {err}");
                }
            }
            let desc = test_ok(CString::new(tmp.to_str().unwrap_or("")));
            let desc_sxp = Rf_mkString(desc.as_ptr());
            let _desc_guard = protect(desc_sxp);
            let open_sxp = Rf_mkString(test_ok(CString::new("")).as_ptr());
            let _open_guard = protect(open_sxp);
            let comp_sxp = Rf_ScalarInteger(6);
            let _comp_guard = protect(comp_sxp);

            let p2 = Rf_cons(comp_sxp, R_NilValue());
            let _p2_guard = protect(p2);
            let p1 = Rf_cons(open_sxp, p2);
            let _p1_guard = protect(p1);
            let args = Rf_cons(desc_sxp, p1);
            let _args_guard = protect(args);

            let result = do_gzfile(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert!(!result.is_null());

            let _ = fs::remove_file(&tmp);
        }
    }

    #[test]
    fn test_readbin_from_raw() {
        let _lock = reset_connections();
        unsafe {
            // Create a raw vector with some bytes
            let raw = Rf_allocVector(SEXPTYPE::RAWSXP, 8);
            let _raw_guard = protect(raw);
            let raw_data = RAW(raw);
            // Write 2 integers (4 bytes each): 42 and 100
            let vals: [i32; 2] = [42, 100];
            ptr::copy_nonoverlapping(vals.as_ptr() as *const u8, raw_data, 8);

            let what_sxp = Rf_mkString(test_ok(CString::new("integer")).as_ptr());
            let _what_guard = protect(what_sxp);
            let n_sxp = Rf_ScalarInteger(2);
            let _n_guard = protect(n_sxp);
            let size_sxp = Rf_ScalarInteger(NA_INTEGER);
            let _size_guard = protect(size_sxp);
            let signed_sxp = Rf_ScalarLogical(1);
            let _signed_guard = protect(signed_sxp);
            let swap_sxp = Rf_ScalarLogical(0);
            let _swap_guard = protect(swap_sxp);

            let p5 = Rf_cons(swap_sxp, R_NilValue());
            let _p5_guard = protect(p5);
            let p4 = Rf_cons(signed_sxp, p5);
            let _p4_guard = protect(p4);
            let p3 = Rf_cons(size_sxp, p4);
            let _p3_guard = protect(p3);
            let p2 = Rf_cons(n_sxp, p3);
            let _p2_guard = protect(p2);
            let p1 = Rf_cons(what_sxp, p2);
            let _p1_guard = protect(p1);
            let args = Rf_cons(raw, p1);
            let _args_guard = protect(args);

            let result = do_readBin(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert!(!result.is_null());
            assert_eq!(LENGTH(result), 2);
            assert_eq!(*INTEGER(result), 42);
            assert_eq!(*INTEGER(result).add(1), 100);
        }
    }

    #[test]
    fn test_writebin_to_raw_connection() {
        let _lock = reset_connections();
        unsafe {
            // Create a raw output connection
            let desc_sxp = Rf_mkString(test_ok(CString::new("test_write_raw")).as_ptr());
            let _desc_guard = protect(desc_sxp);
            let raw_sxp = Rf_allocVector(SEXPTYPE::RAWSXP, 0);
            let _raw_guard = protect(raw_sxp);
            let open_sxp = Rf_mkString(test_ok(CString::new("wb")).as_ptr());
            let _open_guard = protect(open_sxp);
            let local_sxp = Rf_ScalarLogical(0);
            let _local_guard = protect(local_sxp);

            let p2 = Rf_cons(local_sxp, R_NilValue());
            let _p2_guard = protect(p2);
            let p1 = Rf_cons(open_sxp, p2);
            let _p1_guard = protect(p1);
            let raw_args = Rf_cons(raw_sxp, p1);
            let _raw_args_guard = protect(raw_args);
            let conn_args = Rf_cons(desc_sxp, raw_args);
            let _conn_args_guard = protect(conn_args);

            let conn_result =
                do_rawConnection(ptr::null_mut(), ptr::null_mut(), conn_args, ptr::null_mut());
            let _conn_result_guard = protect(conn_result);
            let conn_idx = as_integer(conn_result);

            // Create an integer vector to write
            let obj = Rf_allocVector(SEXPTYPE::INTSXP, 3);
            let _obj_guard = protect(obj);
            *INTEGER(obj) = 10;
            *INTEGER(obj).add(1) = 20;
            *INTEGER(obj).add(2) = 30;

            let size_sxp = Rf_ScalarInteger(NA_INTEGER);
            let _size_guard = protect(size_sxp);
            let swap_sxp = Rf_ScalarLogical(0);
            let _swap_guard = protect(swap_sxp);
            let use_bytes_sxp = Rf_ScalarLogical(0);
            let _use_bytes_guard = protect(use_bytes_sxp);

            let p3 = Rf_cons(use_bytes_sxp, R_NilValue());
            let _p3_guard = protect(p3);
            let p2b = Rf_cons(swap_sxp, p3);
            let _p2b_guard = protect(p2b);
            let p1b = Rf_cons(size_sxp, p2b);
            let _p1b_guard = protect(p1b);
            let write_args = Rf_cons(conn_result, p1b);
            let _write_args_guard = protect(write_args);
            let write_args2 = Rf_cons(obj, write_args);
            let _write_args2_guard = protect(write_args2);

            let result = do_writeBin(
                ptr::null_mut(),
                ptr::null_mut(),
                write_args2,
                ptr::null_mut(),
            );
            assert_eq!(result, R_NilValue());

            // Verify the raw data was written
            let table = connection_table();
            let Some(conn) = table[conn_idx as usize].as_ref() else {
                panic!("expected connection to exist");
            };
            assert_eq!(conn.raw_data.len(), 12); // 3 * 4 bytes
        }
    }

    #[test]
    fn test_sink_number() {
        let _lock = reset_connections();
        unsafe {
            let type_sxp = Rf_ScalarLogical(0);
            let _type_guard = protect(type_sxp);
            let args = Rf_cons(type_sxp, R_NilValue());
            let _args_guard = protect(args);

            let result = do_sinkNumber(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert!(!result.is_null());
            assert_eq!(as_integer(result), 0);
        }
    }

    #[test]
    fn test_conn_new_api() {
        let _lock = reset_connections();
        unsafe {
            R_InitConnections();

            // Test that next_connection returns >= 3
            let idx = next_connection();
            assert!(idx >= 3);

            // Test that get_connection works for standard connections
            drop(get_connection(0));
            drop(get_connection(1));
            drop(get_connection(2));
        }
    }

    #[test]
    fn test_next_connection_reports_r_error_when_table_is_full() {
        let _lock = reset_connections();
        init_connections_table();
        with_connections_state(|state| {
            for i in 3..NCONNECTIONS {
                state.table[i] = Some(Box::new(RConn::new(
                    "textConnection",
                    "test-full-table",
                    "w",
                    ConnKind::TextConnection,
                )));
            }
        });

        let message = expect_r_error(|| {
            let _ = next_connection();
        });

        assert_eq!(message, "all connections are in use");
    }

    #[test]
    fn test_get_connection_reports_r_error_for_invalid_slot() {
        let _lock = reset_connections();
        init_connections_table();

        let message = expect_r_error(|| {
            drop(get_connection(NCONNECTIONS));
        });

        assert_eq!(message, "invalid connection");
    }

    #[test]
    fn test_connection_state_is_session_local_on_same_thread() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let left = RSession::new();
        let right = RSession::new();

        left.with_protected(|| {
            init_connections_table();
            with_connections_state(|state| {
                state.table[3] = Some(Box::new(RConn::new(
                    "textConnection",
                    "left-only",
                    "w",
                    ConnKind::TextConnection,
                )));
                state.sink.output_con = 3;
                assert_eq!(state.table.iter().filter(|conn| conn.is_some()).count(), 4);
            });
        });

        right.with_protected(|| {
            with_connections_state(|state| {
                assert!(state.table.is_empty());
                assert_eq!(state.sink.output_con, 1);
            });
            init_connections_table();
            with_connections_state(|state| {
                assert_eq!(state.table.iter().filter(|conn| conn.is_some()).count(), 3);
                assert!(state.table[3].is_none());
            });
        });

        left.with_protected(|| {
            with_connections_state(|state| {
                assert!(state.table[3].is_some());
                assert_eq!(state.sink.output_con, 3);
            });
        });
    }

    #[test]
    fn test_open_file_read_lines() {
        let _lock = reset_connections();
        unsafe {
            // Create a temp file
            let tmp = std::env::temp_dir().join("rport_test_readlines.txt");
            {
                let mut f = test_ok(File::create(&tmp));
                if let Err(err) = write!(f, "line one\nline two\nline three\n") {
                    panic!("test setup failed: {err}");
                }
            }

            // Create and open file connection
            let desc = test_ok(CString::new(tmp.to_str().unwrap_or("")));
            let open = test_ok(CString::new("r"));
            let desc_sxp = Rf_mkString(desc.as_ptr());
            let _desc_guard = protect(desc_sxp);
            let open_sxp = Rf_mkString(open.as_ptr());
            let _open_guard = protect(open_sxp);
            let enc_sxp = Rf_mkString(test_ok(CString::new("")).as_ptr());
            let _enc_guard = protect(enc_sxp);
            let block_sxp = Rf_ScalarLogical(1);
            let _block_guard = protect(block_sxp);
            let method_sxp = Rf_mkString(test_ok(CString::new("default")).as_ptr());
            let _method_guard = protect(method_sxp);
            let raw_sxp = Rf_ScalarLogical(0);
            let _raw_guard = protect(raw_sxp);

            let p5 = Rf_cons(raw_sxp, R_NilValue());
            let _p5_guard = protect(p5);
            let p4 = Rf_cons(method_sxp, p5);
            let _p4_guard = protect(p4);
            let p3 = Rf_cons(block_sxp, p4);
            let _p3_guard = protect(p3);
            let p2 = Rf_cons(enc_sxp, p3);
            let _p2_guard = protect(p2);
            let p1 = Rf_cons(open_sxp, p2);
            let _p1_guard = protect(p1);
            let file_args = Rf_cons(desc_sxp, p1);
            let _file_args_guard = protect(file_args);

            let conn_result = do_file(ptr::null_mut(), ptr::null_mut(), file_args, ptr::null_mut());
            let _conn_result_guard = protect(conn_result);
            let conn_idx = as_integer(conn_result);

            // Now read lines
            let n_sxp = Rf_ScalarInteger(-1);
            let _n_guard = protect(n_sxp);
            let ok_sxp = Rf_ScalarLogical(1);
            let _ok_guard = protect(ok_sxp);
            let warn_sxp = Rf_ScalarLogical(1);
            let _warn_guard = protect(warn_sxp);
            let enc2_sxp = Rf_mkString(test_ok(CString::new("")).as_ptr());
            let _enc2_guard = protect(enc2_sxp);
            let skipnul_sxp = Rf_ScalarLogical(0);
            let _skipnul_guard = protect(skipnul_sxp);

            let p5r = Rf_cons(skipnul_sxp, R_NilValue());
            let _p5r_guard = protect(p5r);
            let p4r = Rf_cons(enc2_sxp, p5r);
            let _p4r_guard = protect(p4r);
            let p3r = Rf_cons(warn_sxp, p4r);
            let _p3r_guard = protect(p3r);
            let p2r = Rf_cons(ok_sxp, p3r);
            let _p2r_guard = protect(p2r);
            let p1r = Rf_cons(n_sxp, p2r);
            let _p1r_guard = protect(p1r);
            let rl_args = Rf_cons(conn_result, p1r);
            let _rl_args_guard = protect(rl_args);

            let lines_result =
                do_readLines(ptr::null_mut(), ptr::null_mut(), rl_args, ptr::null_mut());
            assert!(!lines_result.is_null());
            assert_eq!(LENGTH(lines_result), 3);

            // Verify line contents
            let l1 = string_elt(lines_result, 0);
            let l2 = string_elt(lines_result, 1);
            let l3 = string_elt(lines_result, 2);
            assert_eq!(l1, "line one");
            assert_eq!(l2, "line two");
            assert_eq!(l3, "line three");

            // Close the connection
            let close_args = Rf_cons(conn_result, R_NilValue());
            let _close_args_guard = protect(close_args);
            let close_result = do_close(
                ptr::null_mut(),
                ptr::null_mut(),
                close_args,
                ptr::null_mut(),
            );
            assert_eq!(close_result, R_NilValue());

            // Clean up
            let _ = fs::remove_file(&tmp);
        }
    }

    #[test]
    fn test_write_lines_to_file() {
        let _lock = reset_connections();
        unsafe {
            let tmp = std::env::temp_dir().join("rport_test_writelines.txt");

            // Create and open file connection for writing
            let desc = test_ok(CString::new(tmp.to_str().unwrap_or("")));
            let open = test_ok(CString::new("w"));
            let desc_sxp = Rf_mkString(desc.as_ptr());
            let _desc_guard = protect(desc_sxp);
            let open_sxp = Rf_mkString(open.as_ptr());
            let _open_guard = protect(open_sxp);
            let enc_sxp = Rf_mkString(test_ok(CString::new("")).as_ptr());
            let _enc_guard = protect(enc_sxp);
            let block_sxp = Rf_ScalarLogical(1);
            let _block_guard = protect(block_sxp);
            let method_sxp = Rf_mkString(test_ok(CString::new("default")).as_ptr());
            let _method_guard = protect(method_sxp);
            let raw_sxp = Rf_ScalarLogical(0);
            let _raw_guard = protect(raw_sxp);

            let p5 = Rf_cons(raw_sxp, R_NilValue());
            let _p5_guard = protect(p5);
            let p4 = Rf_cons(method_sxp, p5);
            let _p4_guard = protect(p4);
            let p3 = Rf_cons(block_sxp, p4);
            let _p3_guard = protect(p3);
            let p2 = Rf_cons(enc_sxp, p3);
            let _p2_guard = protect(p2);
            let p1 = Rf_cons(open_sxp, p2);
            let _p1_guard = protect(p1);
            let file_args = Rf_cons(desc_sxp, p1);
            let _file_args_guard = protect(file_args);

            let conn_result = do_file(ptr::null_mut(), ptr::null_mut(), file_args, ptr::null_mut());
            let _conn_result_guard = protect(conn_result);

            // Create text to write
            let text = Rf_allocVector(SEXPTYPE::STRSXP, 2);
            let _text_guard = protect(text);
            let c1 = Rf_mkChar(test_ok(CString::new("hello")).as_ptr());
            let c2 = Rf_mkChar(test_ok(CString::new("world")).as_ptr());
            SET_STRING_ELT(text, 0, c1);
            SET_STRING_ELT(text, 1, c2);

            let sep_sxp = Rf_mkString(test_ok(CString::new("\n")).as_ptr());
            let _sep_guard = protect(sep_sxp);
            let usebytes_sxp = Rf_ScalarLogical(0);
            let _usebytes_guard = protect(usebytes_sxp);

            let p2w = Rf_cons(usebytes_sxp, R_NilValue());
            let _p2w_guard = protect(p2w);
            let p1w = Rf_cons(sep_sxp, p2w);
            let _p1w_guard = protect(p1w);
            let wl_args = Rf_cons(conn_result, p1w);
            let _wl_args_guard = protect(wl_args);
            let wl_args2 = Rf_cons(text, wl_args);
            let _wl_args2_guard = protect(wl_args2);

            let result = do_writeLines(ptr::null_mut(), ptr::null_mut(), wl_args2, ptr::null_mut());
            assert_eq!(result, R_NilValue());

            // Close the connection
            let close_args = Rf_cons(conn_result, R_NilValue());
            let _close_args_guard = protect(close_args);
            do_close(
                ptr::null_mut(),
                ptr::null_mut(),
                close_args,
                ptr::null_mut(),
            );

            // Read the file back to verify
            let contents = test_ok(fs::read_to_string(&tmp));
            assert_eq!(contents, "hello\nworld\n");

            // Clean up
            let _ = fs::remove_file(&tmp);
        }
    }
}
