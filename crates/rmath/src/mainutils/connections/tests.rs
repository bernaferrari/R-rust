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

        let result = do_textConnection(ptr::null_mut(), ptr::null_mut(), args2, ptr::null_mut());
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

        let result = do_showConnections(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
        assert!(!result.is_null());
        assert!(LENGTH(result) >= 3);
    }
}

#[test]
fn test_do_gzfile_create() {
    let _lock = reset_connections();
    unsafe {
        let tmp = std::env::temp_dir().join("rport_test_gz.txt");
        let desc = test_ok(CString::new(tmp.to_str().unwrap_or("")));
        let desc_sxp = Rf_mkString(desc.as_ptr());
        let _desc_guard = protect(desc_sxp);
        let open_sxp = Rf_mkString(test_ok(CString::new("wb")).as_ptr());
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

        let lines_result = do_readLines(ptr::null_mut(), ptr::null_mut(), rl_args, ptr::null_mut());
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
