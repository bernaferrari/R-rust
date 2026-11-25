use super::*;

// ---------------------------------------------------------------------------
// Connection basics (simplified)
// ---------------------------------------------------------------------------





/// R's `flush.console()` — flush pending console output.
pub fn do_flush_console(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    {
        crate::unix::system::R_FlushConsole();
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// Extended connection constructors
// ---------------------------------------------------------------------------




/// R's `socketConnection(host, port, open, blocking, server, encoding)` — socket connection.
/// Simplified: stub that returns NULL.
pub fn do_socketConnection(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let _host = CAR(args);
    let _port = CAR(CDR(args));
    // Socket connections not yet fully supported
    crate::mainutils::connections::do_sockConnection(_call, _op, args, _rho)
}

// ---------------------------------------------------------------------------
// Connection queries and operations
// ---------------------------------------------------------------------------

/// R's `isOpen(con, rw)` — check if a connection is open.
/// Simplified: delegates to connections.rs.
pub fn do_isOpen(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    crate::mainutils::connections::do_isopen(_call, _op, args, _rho)
}

/// R's `isIncomplete(con)` — check if a connection has incomplete read.
/// Simplified: delegates to connections.rs.
pub fn do_isIncomplete(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    crate::mainutils::connections::do_isincomplete(_call, _op, args, _rho)
}

/// R's `isSeekable(con)` — check if a connection supports seeking.
/// Delegates to the session-owned connection table.
pub fn do_isSeekable(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    crate::mainutils::connections::do_isseekable(_call, _op, args, _rho)
}





// ---------------------------------------------------------------------------
// Binary I/O
// ---------------------------------------------------------------------------


