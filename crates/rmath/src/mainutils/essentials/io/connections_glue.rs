use super::*;

// ---------------------------------------------------------------------------
// Connection basics (simplified)
// ---------------------------------------------------------------------------

/// R's `file(description)` — create a file connection.
/// Delegates to the session-owned connection table.
pub unsafe fn do_file(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::connections::do_file(_call, _op, args, _rho) }
}

/// R's `url(description)` — create a URL connection.
/// Delegates to the session-owned connection table.
pub unsafe fn do_url(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::connections::do_url(_call, _op, args, _rho) }
}

/// R's `close(con)` — close a connection.
/// Delegates to the session-owned connection table.
pub unsafe fn do_close(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::connections::do_close(_call, _op, args, _rho) }
}

/// R's `flush(con)` — flush a connection.
/// Delegates to the session-owned connection table.
pub unsafe fn do_flush(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::connections::do_flush(_call, _op, args, _rho) }
}

/// R's `flush.console()` — flush pending console output.
pub unsafe fn do_flush_console(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        crate::unix::system::R_FlushConsole();
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// Extended connection constructors
// ---------------------------------------------------------------------------

/// R's `gzfile(description, open, encoding, compression)` — gzip connection.
/// Simplified: delegates to connections.rs when available, else returns description.
pub unsafe fn do_gzfile(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let desc = CAR(args);
        if desc.is_null() || desc == R_NilValue() {
            return R_NilValue();
        }
        // Delegate to connections.rs full implementation
        crate::mainutils::connections::do_gzfile(_call, _op, args, _rho)
    }
}

/// R's `pipe(description, open, encoding)` — pipe connection.
/// Simplified: delegates to connections.rs when available, else returns description.
pub unsafe fn do_pipe(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let desc = CAR(args);
        if desc.is_null() || desc == R_NilValue() {
            return R_NilValue();
        }
        // Delegate to connections.rs full implementation
        crate::mainutils::connections::do_pipe(_call, _op, args, _rho)
    }
}

/// R's `fifo(description, open, blocking)` — FIFO connection.
/// Simplified: delegates to connections.rs when available, else returns description.
pub unsafe fn do_fifo(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let desc = CAR(args);
        if desc.is_null() || desc == R_NilValue() {
            return R_NilValue();
        }
        // Delegate to connections.rs full implementation
        crate::mainutils::connections::do_fifo(_call, _op, args, _rho)
    }
}

/// R's `socketConnection(host, port, open, blocking, server, encoding)` — socket connection.
/// Simplified: stub that returns NULL.
pub unsafe fn do_socketConnection(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let _host = CAR(args);
        let _port = CAR(CDR(args));
        // Socket connections not yet fully supported
        crate::mainutils::connections::do_sockConnection(_call, _op, args, _rho)
    }
}

// ---------------------------------------------------------------------------
// Connection queries and operations
// ---------------------------------------------------------------------------

/// R's `isOpen(con, rw)` — check if a connection is open.
/// Simplified: delegates to connections.rs.
pub unsafe fn do_isOpen(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::connections::do_isopen(_call, _op, args, _rho) }
}

/// R's `isIncomplete(con)` — check if a connection has incomplete read.
/// Simplified: delegates to connections.rs.
pub unsafe fn do_isIncomplete(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::connections::do_isincomplete(_call, _op, args, _rho) }
}

/// R's `isSeekable(con)` — check if a connection supports seeking.
/// Delegates to the session-owned connection table.
pub unsafe fn do_isSeekable(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::connections::do_isseekable(_call, _op, args, _rho) }
}

/// R's `seek(con, where, origin, rw)` — seek in a connection.
/// Simplified: delegates to connections.rs.
pub unsafe fn do_seek(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::connections::do_seek(_call, _op, args, _rho) }
}

/// R's `pushBack(lines, con, newLine)` — push back lines to a connection.
/// Simplified: no-op stub.
pub unsafe fn do_pushBack(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::connections::do_pushBack(_call, _op, args, _rho) }
}

/// R's `pushBackClear(con)` — clear push back buffer.
pub unsafe fn do_pushBackClear(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::connections::do_pushBackClear(_call, _op, args, _rho) }
}

/// R's `pushBackLength(con)` — get push back buffer length.
/// Simplified: returns 0.
pub unsafe fn do_pushBackLength(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::connections::do_pushBackLength(_call, _op, args, _rho) }
}

// ---------------------------------------------------------------------------
// Binary I/O
// ---------------------------------------------------------------------------

/// R's `readBin(con, what, n, size, signed, endian)` — read binary data.
/// Delegates to connections.rs for full implementation.
pub unsafe fn do_readBin(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::connections::do_readBin(_call, _op, args, _rho) }
}

/// R's `writeBin(object, con, size, endian, useBytes)` — write binary data.
/// Delegates to connections.rs for full implementation.
pub unsafe fn do_writeBin(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::connections::do_writeBin(_call, _op, args, _rho) }
}
