use super::*;

/// R's `readChar(con, nchars)` — read characters from connection.
pub unsafe fn do_readChar(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let con_arg = CAR(args);
        let nchars_arg = CAR(CDR(args));
        let nchars = real_or_default(nchars_arg, -1.0) as i64;

        if inherits_class(con_arg, "connection") {
            let connection = connection_index(con_arg);
            let text = read_chars_from_connection(connection, nchars);
            return Rf_mkString(CString::new(text).unwrap_or_default().as_ptr());
        }

        let path = elt_to_string(con_arg, 0);
        let bytes =
            crate::mainutils::browser_files::read_bytes_or_host(&path).unwrap_or_else(|e| {
                base_error(format!("cannot read file '{}': {}", path, e));
            });
        let take = if nchars >= 0 {
            (nchars as usize).min(bytes.len())
        } else {
            bytes.len()
        };
        if bytes[..take].contains(&0) {
            crate::mainutils::errors::Rf_warning(c"truncating string with embedded nuls".as_ptr());
        }
        let take = bytes[..take].iter().position(|b| *b == 0).unwrap_or(take);
        let result = String::from_utf8_lossy(&bytes[..take]).into_owned();
        Rf_mkString(CString::new(result).unwrap_or_default().as_ptr())
    }
}

unsafe fn read_chars_from_connection(connection: c_int, nchars: i64) -> String {
    let opened_here = unsafe {
        let index = crate::mainutils::connections::checked_connection_index(connection);
        let mut table = crate::mainutils::connections::connection_table();
        let Some(conn) = table[index].as_mut() else {
            crate::mainutils::connections::r_error("invalid connection");
        };
        if conn.isopen {
            false
        } else {
            let saved_mode = conn.mode.clone();
            let result = match conn.kind {
                crate::mainutils::connections::ConnKind::GzFile => {
                    crate::mainutils::connections::open_gz_conn(conn, "rb")
                }
                crate::mainutils::connections::ConnKind::BzFile => {
                    crate::mainutils::connections::open_bz_conn(conn, "rb")
                }
                crate::mainutils::connections::ConnKind::XzFile => {
                    crate::mainutils::connections::open_xz_conn(conn, "rb")
                }
                crate::mainutils::connections::ConnKind::File => {
                    crate::mainutils::connections::open_maybe_compressed(conn, "rb")
                }
                _ => Err(std::io::Error::other("connection is not open")),
            };
            conn.mode = saved_mode;
            if let Err(e) = result {
                crate::mainutils::connections::r_error(&format!("cannot open the connection: {e}"));
            }
            true
        }
    };
    let mut bytes = Vec::new();
    if nchars >= 0 {
        for _ in 0..nchars {
            let byte = crate::mainutils::connections::connection_fgetc(connection);
            if byte < 0 {
                break;
            }
            if byte == 0 {
                unsafe {
                    crate::mainutils::errors::Rf_warning(
                        c"truncating string with embedded nuls".as_ptr(),
                    )
                };
                break;
            }
            bytes.push(byte as u8);
        }
    } else {
        loop {
            let byte = crate::mainutils::connections::connection_fgetc(connection);
            if byte < 0 {
                break;
            }
            bytes.push(byte as u8);
        }
    }
    if opened_here {
        let instance = crate::sexp::instance::with_required_current_instance(|instance| instance);
        crate::mainutils::connections::close_connection_at_in(instance, connection);
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// R's `writeChar(object, con, nchars)` — write characters to connection.
pub unsafe fn do_writeChar(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let object_arg = CAR(args);
        let con_arg = CAR(CDR(args));
        let nchars_arg = CAR(CDR(CDR(args)));
        let eos_arg = CAR(CDR(CDR(CDR(args))));

        let mut text = elt_to_string(object_arg, 0);
        let nchars = real_or_default(nchars_arg, text.len() as f64) as i64;
        if nchars >= 0 && (nchars as usize) < text.len() {
            text.truncate(nchars as usize);
        }
        // GNU appends only the `eos` terminator (default "" writes
        // nothing); `eos = NULL` writes no terminator at all, so a
        // trailing NUL must not appear in the output.
        if !eos_arg.is_null() && eos_arg != R_NilValue() && TYPEOF(eos_arg) == SEXPTYPE::STRSXP {
            text.push_str(&elt_to_string(eos_arg, 0));
        }

        if inherits_class(con_arg, "connection") {
            let connection = connection_index(con_arg);
            crate::mainutils::connections::connection_write_bytes(connection, text.as_bytes());
        } else {
            let path = elt_to_string(con_arg, 0);
            if let Err(e) =
                crate::mainutils::browser_files::write_text_or_host(&path, text.as_bytes())
            {
                base_error(format!("cannot write file '{}': {}", path, e));
            }
        }

        R_NilValue()
    }
}

pub(crate) unsafe fn connection_index(con: SEXP) -> c_int {
    unsafe {
        if con.is_null()
            || con == R_NilValue()
            || TYPEOF(con) != SEXPTYPE::INTSXP
            || LENGTH(con) < 1
        {
            base_error("invalid connection");
        }
        *INTEGER(con)
    }
}
