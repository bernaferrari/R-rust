//! Connection state: types, per-session table, byte-level read/write plumbing, init, `R_GetConnection` — extracted verbatim from the former single-file module.
#![allow(unused_imports)]
use super::*;
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
pub const R_EOF: c_int = -1;

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
    pub fn new(class: &str, description: &str, mode: &str, kind: ConnKind) -> Self {
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
pub struct SinkState {
    /// Sink connection indices.
    pub sink_cons: Vec<c_int>,
    /// Whether each sink should be closed on exit.
    pub sink_close: Vec<bool>,
    /// Whether each sink is a split (tee) sink.
    pub sink_split: Vec<bool>,
    /// Number of active sinks.
    pub sink_number: usize,
    /// Output connection index.
    pub output_con: c_int,
    /// Error connection index.
    pub error_con: c_int,
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
pub struct ConnectionsState {
    pub table: Vec<Option<Box<RConn>>>,
    pub sink: SinkState,
}

impl Default for ConnectionsState {
    fn default() -> Self {
        ConnectionsState {
            table: Vec::new(),
            sink: SinkState::default(),
        }
    }
}

pub struct ConnectionTableGuard {
    pub table: *mut Vec<Option<Box<RConn>>>,
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

pub struct SinkStateGuard {
    pub sink: *mut SinkState,
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
pub fn with_connections_state<F, R>(f: F) -> R
where
    F: FnOnce(&mut ConnectionsState) -> R,
{
    with_required_current_instance(|instance| f(&mut instance.connections_state))
}

pub fn connection_table() -> ConnectionTableGuard {
    init_connections_table();
    with_connections_state(|state| ConnectionTableGuard {
        table: &mut state.table,
    })
}

pub fn sink_state() -> SinkStateGuard {
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
pub fn init_connections_table() {
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
pub fn next_connection() -> usize {
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
pub fn get_connection(n: usize) -> ConnectionTableGuard {
    init_connections_table();
    let table = connection_table();
    if n >= table.len() || table[n].is_none() {
        r_error("invalid connection");
    }
    table
}

/// Get a mutable reference to a connection by index.
pub fn get_connection_mut(n: usize) {
    init_connections_table();
    let _table = connection_table();
    if n >= _table.len() || _table[n].is_none() {
        r_error("invalid connection");
    }
    // The caller should use the table directly for mutation
}

pub fn checked_connection_index(n: c_int) -> usize {
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

pub fn pop_pushback_byte(conn: &mut RConn) -> Option<u8> {
    while let Some(buf) = conn.pushback.last_mut() {
        if let Some(byte) = buf.pop() {
            return Some(byte);
        }
        conn.pushback.pop();
    }
    None
}

pub fn nul_normalized_line(mut line: Vec<u8>, skip_nul: bool) -> String {
    if line.ends_with(b"\r") {
        line.pop();
    }
    if skip_nul {
        line.retain(|byte| *byte != 0);
    } else if let Some(pos) = line.iter().position(|byte| *byte == 0) {
        line.truncate(pos);
    }
    String::from_utf8_lossy(&line).to_string()
}

pub fn nul_normalized_lines(bytes: &[u8], limit: usize, skip_nul: bool) -> Vec<String> {
    let mut lines = Vec::new();
    let mut start = 0;
    while start < bytes.len() && lines.len() < limit {
        let end = bytes[start..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(bytes.len(), |offset| start + offset);
        lines.push(nul_normalized_line(bytes[start..end].to_vec(), skip_nul));
        if end == bytes.len() {
            break;
        }
        start = end + 1;
    }
    lines
}

pub fn read_pushback_line(conn: &mut RConn, skip_nul: bool) -> Option<String> {
    let mut line = Vec::new();
    while let Some(byte) = pop_pushback_byte(conn) {
        if byte == b'\n' {
            return Some(nul_normalized_line(line, skip_nul));
        }
        if byte != b'\r' {
            line.push(byte);
        }
    }

    (!line.is_empty()).then(|| nul_normalized_line(line, skip_nul))
}

pub fn read_raw_line(conn: &mut RConn, skip_nul: bool) -> Option<String> {
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
    Some(nul_normalized_line(
        conn.raw_data[start..end].to_vec(),
        skip_nul,
    ))
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

pub fn write_bytes_to_conn(conn: &mut RConn, bytes: &[u8]) {
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
// Connection initialization
// ---------------------------------------------------------------------------

/// Initialize the connection system.
pub unsafe fn R_InitConnections() {
    init_connections_table();
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
