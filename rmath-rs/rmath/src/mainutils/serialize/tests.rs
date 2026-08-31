use super::*;


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

    use crate::sexp::envir::R_NewHashedEnv;
    use crate::sexp::envir::defineVar;
    use crate::sexp::globals::{
        R_BaseEnv, R_EmptyEnv, R_GlobalEnv, R_MissingArg, R_NaString, R_UnboundValue,
    };
    use crate::sexp::instance::{RInstance, clear_current_instance, set_current_instance};
    use std::ffi::CString;
    use std::mem;
    use std::os::raw::c_void;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn must<T, E: std::fmt::Debug>(r: Result<T, E>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => panic!("test failed: {e:?}"),
        }
    }

    fn make_raw(bytes: &[u8]) -> SEXP {
        let raw = unsafe { Rf_allocVector3(SEXPTYPE::RAWSXP, bytes.len() as R_xlen_t) };
        if !raw.is_null() && !bytes.is_empty() {
            unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), RAW(raw), bytes.len()) };
        }
        raw
    }

    fn make_string_scalar(value: &str) -> SEXP {
        let c_value = CString::new(value).unwrap_or_default();
        unsafe { Rf_mkString(c_value.as_ptr()) }
    }

    fn make_string_vector(value: &str) -> SEXP {
        let vec = unsafe { Rf_allocVector3(SEXPTYPE::STRSXP, 1) };
        unsafe {
            SET_STRING_ELT(
                vec,
                0,
                Rf_mkChar(CString::new(value).unwrap_or_default().as_ptr()),
            );
        }
        vec
    }

    #[test]
    fn serialize_runtime_state_is_session_local() {
        let mut first = RInstance::new();
        unsafe {
            set_current_instance(&mut first);
        }
        increment_read_item_depth();
        assert_eq!(read_item_depth_for_test(), 1);

        let mut second = RInstance::new();
        unsafe {
            set_current_instance(&mut second);
        }
        assert_eq!(read_item_depth_for_test(), 0);
        decrement_read_item_depth();
        assert_eq!(read_item_depth_for_test(), 0);
        increment_read_item_depth();
        increment_read_item_depth();
        assert_eq!(read_item_depth_for_test(), 2);

        unsafe {
            set_current_instance(&mut first);
        }
        assert_eq!(read_item_depth_for_test(), 1);
        decrement_read_item_depth();
        assert_eq!(read_item_depth_for_test(), 0);

        clear_current_instance();
    }

    fn make_na_string_vector() -> SEXP {
        let vec = unsafe { Rf_allocVector3(SEXPTYPE::STRSXP, 1) };
        unsafe { SET_STRING_ELT(vec, 0, R_NaString()) };
        vec
    }

    fn make_args(items: &[SEXP]) -> SEXP {
        let mut tail = unsafe { R_NilValue() };
        for item in items.iter().rev().copied() {
            tail = unsafe { Rf_cons(item, tail) };
        }
        tail
    }

    fn temp_path(stem: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        path.push(format!(
            "rport-serialize-{stem}-{}-{nanos}.bin",
            std::process::id()
        ));
        path
    }

    #[test]
    fn test_default_serialize_version() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = defaultSerializeVersion();
            assert_eq!(v, 3);
        }
    }

    #[test]
    fn test_binary_writer_reader_i32() {
        let _session = crate::sexp::session::RSession::new();
        let mut writer = BinaryWriter::new();
        writer.write_i32(42);
        writer.write_i32(-1);
        writer.write_i32(i32::MAX);
        let data = writer.into_vec();
        assert_eq!(data.len(), 12);

        let mut reader = BinaryReader::new(&data);
        assert_eq!(must(reader.read_i32()), 42);
        assert_eq!(must(reader.read_i32()), -1);
        assert_eq!(must(reader.read_i32()), i32::MAX);
    }

    #[test]
    fn test_binary_writer_reader_f64() {
        let _session = crate::sexp::session::RSession::new();
        let mut writer = BinaryWriter::new();
        writer.write_f64(3.14);
        writer.write_f64(-1.0);
        writer.write_f64(0.0);
        let data = writer.into_vec();

        let mut reader = BinaryReader::new(&data);
        assert!((must(reader.read_f64()) - 3.14).abs() < 1e-10);
        assert_eq!(must(reader.read_f64()), -1.0);
        assert_eq!(must(reader.read_f64()), 0.0);
    }

    #[test]
    fn test_binary_writer_reader_bytes() {
        let _session = crate::sexp::session::RSession::new();
        let mut writer = BinaryWriter::new();
        writer.write_byte(0xFF);
        writer.write_byte(0x00);
        writer.write_byte(0x42);
        let data = writer.into_vec();

        let mut reader = BinaryReader::new(&data);
        assert_eq!(must(reader.read_byte()), 0xFF);
        assert_eq!(must(reader.read_byte()), 0x00);
        assert_eq!(must(reader.read_byte()), 0x42);
    }

    #[test]
    fn test_pack_flags() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            // Type only, no flags
            let flags = PackFlags(13, 0, 0, 0, 0);
            assert_eq!(flags, 13);

            // Type with object flag
            let flags = PackFlags(14, 0, 1, 0, 0);
            assert_eq!(flags, 0b1110 | IS_OBJECT_BIT_MASK);

            // Type with attr and tag flags
            let flags = PackFlags(19, 0, 0, 1, 1);
            assert_eq!(flags, 0b10011 | HAS_ATTR_BIT_MASK | HAS_TAG_BIT_MASK);

            // Type with levels
            let flags = PackFlags(10, 3, 0, 0, 0);
            assert_eq!(flags, 0b1010 | (3 << 12));
        }
    }

    #[test]
    fn test_pack_flags_strips_growable_bit_from_vectors() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let flags = PackFlags(SEXPTYPE::VECSXP.as_c_int(), GROWABLE_MASK, 0, 0, 0);
            assert_eq!(flags, SEXPTYPE::VECSXP.as_c_int());

            let closure_flags = PackFlags(SEXPTYPE::CLOSXP.as_c_int(), GROWABLE_MASK, 0, 0, 0);
            assert_eq!(
                closure_flags,
                SEXPTYPE::CLOSXP.as_c_int() | (GROWABLE_MASK << 12)
            );
        }
    }

    #[test]
    fn test_unpack_flags() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let mut ptype = 0i32;
            let mut plevs = 0i32;
            let mut pisobj = 0i32;
            let mut phasattr = 0i32;
            let mut phastag = 0i32;

            let flags = 0b10011 | HAS_ATTR_BIT_MASK | HAS_TAG_BIT_MASK | (2 << 12);
            UnpackFlags(
                flags,
                &mut ptype,
                &mut plevs,
                &mut pisobj,
                &mut phasattr,
                &mut phastag,
            );
            assert_eq!(ptype, 19);
            assert_eq!(plevs, 2);
            assert_eq!(pisobj, 0);
            assert_eq!(phasattr, 1);
            assert_eq!(phastag, 1);
        }
    }

    #[test]
    fn test_decode_version() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let mut v = 0i32;
            let mut p = 0i32;
            let mut s = 0i32;
            DecodeVersion(R_VERSION_450, &mut v, &mut p, &mut s);
            assert_eq!(v, 4);
            assert_eq!(p, 5);
            assert_eq!(s, 0);

            DecodeVersion(R_VERSION_230, &mut v, &mut p, &mut s);
            assert_eq!(v, 2);
            assert_eq!(p, 3);
            assert_eq!(s, 0);

            DecodeVersion(R_VERSION_350, &mut v, &mut p, &mut s);
            assert_eq!(v, 3);
            assert_eq!(p, 5);
            assert_eq!(s, 0);
        }
    }

    #[test]
    fn test_write_hash_table() {
        let _session = crate::sexp::session::RSession::new();
        let mut ht = WriteHashTable::new();
        assert_eq!(ht.count, 0);

        // Getting a non-existent key returns 0
        let fake_ptr = 0x1000 as *mut std::os::raw::c_void as SEXP;
        assert_eq!(ht.get(fake_ptr), 0);

        // Add and retrieve
        ht.add(fake_ptr);
        assert_eq!(ht.count, 1);
        assert_eq!(ht.get(fake_ptr), 1);

        // Add second
        let fake_ptr2 = 0x2000 as *mut std::os::raw::c_void as SEXP;
        ht.add(fake_ptr2);
        assert_eq!(ht.count, 2);
        assert_eq!(ht.get(fake_ptr2), 2);
        assert_eq!(ht.get(fake_ptr), 1);
    }

    #[test]
    fn test_read_ref_table() {
        let _session = crate::sexp::session::RSession::new();
        let mut rt = ReadRefTable::new();
        let fake1 = 0x1000 as *mut std::os::raw::c_void as SEXP;
        let fake2 = 0x2000 as *mut std::os::raw::c_void as SEXP;

        rt.add(fake1);
        rt.add(fake2);
        assert_eq!(must(rt.get(1)), fake1);
        assert_eq!(must(rt.get(2)), fake2);
        assert!(rt.get(3).is_err());
        assert!(rt.get(0).is_err());
    }

    #[test]
    fn test_c_read_ref_table_helpers() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let table = MakeReadRefTable();
            let a = Rf_ScalarInteger(11);
            let b = Rf_ScalarReal(2.5);
            AddReadRef(table, a);
            AddReadRef(table, b);
            assert_eq!(GetReadRef(table, 1), a);
            assert_eq!(GetReadRef(table, 2), b);
        }
    }

    #[test]
    fn test_c_hash_table_helpers() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let ht = MakeHashTable();
            let a = Rf_ScalarInteger(1);
            let b = Rf_ScalarInteger(2);
            assert_eq!(HashGet(a, ht), 0);
            HashAdd(a, ht);
            HashAdd(b, ht);
            assert!(HashGet(a, ht) > 0);
            assert!(HashGet(b, ht) > 0);
        }
    }

    #[test]
    fn test_writebc_readbc_round_trip() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let mut out_stream: R_outpstream_st = mem::zeroed();
            let mut out_buf = membuf_st {
                size: 0,
                count: 0,
                buf: ptr::null_mut(),
            };
            InitMemOutPStream(
                &mut out_stream,
                &mut out_buf,
                R_pstream_format_t::R_pstream_binary_format,
                3,
                None,
                R_NilValue(),
            );
            let value = Rf_ScalarInteger(77);
            WriteBC(value, ptr::null_mut(), &mut out_stream);
            let raw = CloseMemOutPStream(&mut out_stream);

            let mut in_stream: R_inpstream_st = mem::zeroed();
            let mut in_buf = membuf_st {
                size: 0,
                count: 0,
                buf: ptr::null_mut(),
            };
            InitMemInPStream(
                &mut in_stream,
                &mut in_buf,
                RAW(raw) as *mut c_void,
                XLENGTH(raw) as R_size_t,
                None,
                R_NilValue(),
            );
            let got = ReadBC(ptr::null_mut(), &mut in_stream);
            assert_eq!(TYPEOF(got), SEXPTYPE::INTSXP);
            assert_eq!(LENGTH(got), 1);
            assert_eq!(*INTEGER(got), 77);
        }
    }

    #[test]
    fn test_conn_stream_file_callbacks() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let fp = libc::tmpfile();
            assert!(!fp.is_null());

            let mut out_stream: R_outpstream_st = mem::zeroed();
            R_InitConnOutPStream(
                &mut out_stream,
                fp as *mut c_void,
                R_pstream_format_t::R_pstream_binary_format,
                3,
                None,
                R_NilValue(),
            );
            assert!(out_stream.OutBytes.is_some());
            let bytes = [1u8, 2, 3, 4];
            out_stream.OutBytes.unwrap()(
                &mut out_stream,
                bytes.as_ptr() as *const c_void,
                bytes.len() as c_int,
            );
            libc::fflush(fp);
            libc::rewind(fp);

            let mut in_stream: R_inpstream_st = mem::zeroed();
            R_InitConnInPStream(
                &mut in_stream,
                fp as *mut c_void,
                R_pstream_format_t::R_pstream_binary_format,
                None,
                R_NilValue(),
            );
            assert!(in_stream.InBytes.is_some());
            let mut got = [0u8; 4];
            in_stream.InBytes.unwrap()(
                &mut in_stream,
                got.as_mut_ptr() as *mut c_void,
                got.len() as c_int,
            );
            assert_eq!(got, bytes);

            libc::fclose(fp);
        }
    }

    #[test]
    fn test_r_write_connection_file_round_trip() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let fp = libc::tmpfile();
            assert!(!fp.is_null());
            let bytes = b"hello";
            let wrote = R_WriteConnection(
                fp as *mut c_void,
                bytes.as_ptr() as *const c_void,
                bytes.len(),
            );
            assert_eq!(wrote, bytes.len());
            libc::fflush(fp);
            libc::fseek(fp, 0, libc::SEEK_END);
            let size = libc::ftell(fp);
            assert_eq!(size, bytes.len() as libc::c_long);
            libc::fclose(fp);
        }
    }

    #[test]
    fn test_instringvec_round_trip() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let src = Rf_allocVector3(SEXPTYPE::STRSXP, 3);
            SET_STRING_ELT(src, 0, Rf_mkChar(c"foo".as_ptr()));
            SET_STRING_ELT(src, 1, R_NaString());
            SET_STRING_ELT(src, 2, Rf_mkChar(c"bar".as_ptr()));

            let mut out_stream: R_outpstream_st = mem::zeroed();
            let mut out_buf = membuf_st {
                size: 0,
                count: 0,
                buf: ptr::null_mut(),
            };
            InitMemOutPStream(
                &mut out_stream,
                &mut out_buf,
                R_pstream_format_t::R_pstream_binary_format,
                3,
                None,
                R_NilValue(),
            );
            let write_ref_table = MakeHashTable();
            OutStringVec(&mut out_stream, src, write_ref_table);
            let raw = CloseMemOutPStream(&mut out_stream);

            let mut in_stream: R_inpstream_st = mem::zeroed();
            let mut in_buf = membuf_st {
                size: 0,
                count: 0,
                buf: ptr::null_mut(),
            };
            InitMemInPStream(
                &mut in_stream,
                &mut in_buf,
                RAW(raw) as *mut c_void,
                XLENGTH(raw) as R_size_t,
                None,
                R_NilValue(),
            );
            in_stream.type_ = R_pstream_format_t::R_pstream_binary_format;

            let read_ref_table = MakeReadRefTable();
            let got = InStringVec(&mut in_stream, read_ref_table);
            assert_eq!(TYPEOF(got), SEXPTYPE::STRSXP);
            assert_eq!(LENGTH(got), 3);
            let g0 = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(got, 0)));
            let g2 = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(got, 2)));
            assert_eq!(g0.to_bytes(), b"foo");
            assert_eq!(STRING_ELT(got, 1), R_NaString());
            assert_eq!(g2.to_bytes(), b"bar");
        }
    }

    #[test]
    fn test_ref_index_packing() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let mut writer = BinaryWriter::new();
            // Small index: packed into single integer
            OutRefIndex(&mut writer, 1);
            let data = writer.into_vec();
            let mut reader = BinaryReader::new(&data);
            let flags = must(reader.read_i32());
            assert_eq!(flags & 0xFF, REFSXP);
            assert_eq!(flags >> 8, 1);
        }
    }

    #[test]
    fn test_constants() {
        let _session = crate::sexp::session::RSession::new();
        assert_eq!(HASHSIZE, 1009);
        assert_eq!(INITIAL_REFREAD_TABLE_SIZE, 128);
        assert_eq!(REFSXP, 255);
        assert_eq!(NC, 100);
        assert_eq!(R_CODESET_MAX, 256);
        assert_eq!(CHUNK_SIZE, 512);
        assert_eq!(R_DEFAULT_SERIALIZE_VERSION, 3);
        assert_eq!(NILVALUE_SXP, 254);
        assert_eq!(GLOBALENV_SXP, 253);
        assert_eq!(MAX_PACKED_INDEX, c_int::MAX >> 8);
    }

    #[test]
    #[should_panic]
    fn test_r_serialize_returns_nil_for_null() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = R_serialize(
                ptr::null_mut(),
                R_NilValue(),
                R_NilValue(),
                R_NilValue(),
                R_NilValue(),
            );
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    #[should_panic]
    fn test_r_unserialize_returns_nil_for_null() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = R_unserialize(R_NilValue(), R_NilValue());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    #[should_panic]
    fn test_r_unserialize_returns_nil_for_empty_raw() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let raw = Rf_allocVector3(SEXPTYPE::RAWSXP, 0);
            let result = R_unserialize(raw, R_NilValue());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    #[should_panic]
    fn test_r_unserialize_returns_nil_for_non_raw() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s = Rf_allocVector3(SEXPTYPE::INTSXP, 1);
            let result = R_unserialize(s, R_NilValue());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    #[should_panic]
    fn test_r_serialize_info_returns_nil() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = R_SerializeInfo(ptr::null_mut());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_r_serialize_info_reports_header() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let value = Rf_ScalarInteger(123);
            let raw = R_serialize(
                value,
                R_NilValue(),
                R_NilValue(),
                R_NilValue(),
                R_NilValue(),
            );
            assert_eq!(TYPEOF(raw), SEXPTYPE::RAWSXP);

            let mut in_stream: R_inpstream_st = mem::zeroed();
            let mut in_buf = membuf_st {
                size: 0,
                count: 0,
                buf: ptr::null_mut(),
            };
            InitMemInPStream(
                &mut in_stream,
                &mut in_buf,
                RAW(raw) as *mut c_void,
                XLENGTH(raw) as R_size_t,
                None,
                R_NilValue(),
            );

            let info = R_SerializeInfo(&mut in_stream);
            assert_eq!(TYPEOF(info), SEXPTYPE::VECSXP);
            assert_eq!(LENGTH(info), 5);

            let names = crate::eval::attrib_core::getAttrib(info, R_NamesSymbol());
            assert_eq!(TYPEOF(names), SEXPTYPE::STRSXP);
            assert_eq!(LENGTH(names), 5);
            let n0 = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, 0)));
            let n1 = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, 1)));
            let n2 = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, 2)));
            let n3 = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, 3)));
            let n4 = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, 4)));
            assert_eq!(n0.to_bytes(), b"version");
            assert_eq!(n1.to_bytes(), b"writer_version");
            assert_eq!(n2.to_bytes(), b"min_reader_version");
            assert_eq!(n3.to_bytes(), b"format");
            assert_eq!(n4.to_bytes(), b"native_encoding");

            let version = VECTOR_ELT(info, 0);
            assert_eq!(TYPEOF(version), SEXPTYPE::INTSXP);
            assert_eq!(*INTEGER(version), 3);

            let writer = VECTOR_ELT(info, 1);
            assert_eq!(TYPEOF(writer), SEXPTYPE::STRSXP);
            let writer_s = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(writer, 0)));
            assert!(writer_s.to_bytes().contains(&b'.'));

            let min_reader = VECTOR_ELT(info, 2);
            assert_eq!(TYPEOF(min_reader), SEXPTYPE::STRSXP);
            let min_reader_s = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(min_reader, 0)));
            assert!(min_reader_s.to_bytes().contains(&b'.'));

            let fmt = VECTOR_ELT(info, 3);
            let fmt_s = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(fmt, 0)));
            assert_eq!(fmt_s.to_bytes(), b"xdr");

            let enc = VECTOR_ELT(info, 4);
            let enc_s = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(enc, 0)));
            assert_eq!(enc_s.to_bytes(), b"");
        }
    }

    #[test]
    fn test_r_serialize_honors_requested_version_2() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let value = Rf_ScalarInteger(123);
            let version = Rf_ScalarInteger(2);
            let raw = R_serialize(value, R_NilValue(), R_NilValue(), version, R_NilValue());
            assert_eq!(TYPEOF(raw), SEXPTYPE::RAWSXP);

            let mut in_stream: R_inpstream_st = mem::zeroed();
            let mut in_buf = membuf_st {
                size: 0,
                count: 0,
                buf: ptr::null_mut(),
            };
            InitMemInPStream(
                &mut in_stream,
                &mut in_buf,
                RAW(raw) as *mut c_void,
                XLENGTH(raw) as R_size_t,
                None,
                R_NilValue(),
            );

            let info = R_SerializeInfo(&mut in_stream);
            assert_eq!(TYPEOF(info), SEXPTYPE::VECSXP);
            assert_eq!(LENGTH(info), 4);
            assert_eq!(*INTEGER(VECTOR_ELT(info, 0)), 2);

            let names = crate::eval::attrib_core::getAttrib(info, R_NamesSymbol());
            assert_eq!(TYPEOF(names), SEXPTYPE::STRSXP);
            assert_eq!(LENGTH(names), 4);
            let n0 = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, 0)));
            let n1 = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, 1)));
            let n2 = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, 2)));
            let n3 = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, 3)));
            assert_eq!(n0.to_bytes(), b"version");
            assert_eq!(n1.to_bytes(), b"writer_version");
            assert_eq!(n2.to_bytes(), b"min_reader_version");
            assert_eq!(n3.to_bytes(), b"format");
        }
    }

    #[test]
    #[should_panic]
    fn test_do_serialize_returns_nil() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = do_serialize(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    #[should_panic]
    fn test_do_serialize_to_conn_returns_nil() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = do_serializeToConn(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    #[should_panic]
    fn test_do_unserialize_from_conn_returns_nil() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = do_unserializeFromConn(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_do_lazy_load_db_flush_returns_nil() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let path = temp_path("flush");
            let path_str = path.to_str().unwrap().to_owned();
            let file = make_string_scalar(&path_str);
            let args = make_args(&[file]);
            let result =
                do_lazyLoadDBflush(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_do_lazy_load_db_insert_and_fetch_round_trip() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let path = temp_path("lazyload");
            let path_str = path.to_str().unwrap().to_owned();
            let file = make_string_scalar(&path_str);
            let value = Rf_ScalarInteger(123);
            let insert_args = make_args(&[
                value,
                file,
                Rf_ScalarLogical(0),
                Rf_ScalarInteger(3),
                R_NilValue(),
            ]);

            let key = do_lazyLoadDBinsertValue(
                ptr::null_mut(),
                ptr::null_mut(),
                insert_args,
                ptr::null_mut(),
            );
            assert_eq!(TYPEOF(key), SEXPTYPE::INTSXP);
            assert_eq!(LENGTH(key), 2);

            let fetch_args = make_args(&[
                key,
                make_string_scalar(&path_str),
                Rf_ScalarInteger(3),
                R_NilValue(),
            ]);
            let fetched = do_lazyLoadDBfetch(
                ptr::null_mut(),
                ptr::null_mut(),
                fetch_args,
                ptr::null_mut(),
            );
            assert_eq!(TYPEOF(fetched), SEXPTYPE::INTSXP);
            assert_eq!(LENGTH(fetched), 1);
            assert_eq!(*INTEGER(fetched), 123);
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn test_do_get_vars_from_frame_returns_values() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let env = R_NewHashedEnv(R_BaseEnv(), 29);
            let sym = Rf_install(c"x".as_ptr());
            let val = Rf_ScalarInteger(42);
            defineVar(sym, val, env);

            let vars = make_string_vector("x");
            let args = make_args(&[vars, env, Rf_ScalarLogical(0)]);
            let result =
                do_getVarsFromFrame(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(TYPEOF(result), SEXPTYPE::VECSXP);
            assert_eq!(LENGTH(result), 1);
            let elt = VECTOR_ELT(result, 0);
            assert_eq!(TYPEOF(elt), SEXPTYPE::INTSXP);
            assert_eq!(*INTEGER(elt), 42);
        }
    }

    #[test]
    fn test_r_compress_decompress_round_trip() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let raw = make_raw(&[0, 1, 2, 3, 4, 5, 6, 7]);
            let mut err: Rboolean = 0;

            let c1 = R_compress1(raw);
            let c1_data = slice::from_raw_parts(RAW(c1), XLENGTH(c1) as usize);
            assert_eq!(parse_swapped_len_prefix(c1_data), Some(8));
            let d1 = R_decompress1(c1, &mut err);
            assert_eq!(err, 0);
            assert_eq!(TYPEOF(d1), SEXPTYPE::RAWSXP);
            assert_eq!(slice::from_raw_parts(RAW(d1), 8), &[0, 1, 2, 3, 4, 5, 6, 7]);

            err = 0;
            let c2 = R_compress2(raw);
            let c2_data = slice::from_raw_parts(RAW(c2), XLENGTH(c2) as usize);
            assert_eq!(parse_swapped_len_prefix(c2_data), Some(8));
            assert!(c2_data.len() >= 5);
            assert!(matches!(c2_data[4], b'2' | b'0'));
            let d2 = R_decompress2(c2, &mut err);
            assert_eq!(err, 0);
            assert_eq!(TYPEOF(d2), SEXPTYPE::RAWSXP);
            assert_eq!(slice::from_raw_parts(RAW(d2), 8), &[0, 1, 2, 3, 4, 5, 6, 7]);

            err = 0;
            let c3 = R_compress3(raw);
            let c3_data = slice::from_raw_parts(RAW(c3), XLENGTH(c3) as usize);
            assert_eq!(parse_swapped_len_prefix(c3_data), Some(8));
            assert!(c3_data.len() >= 5);
            assert!(matches!(c3_data[4], b'Z' | b'0'));
            let d3 = R_decompress3(c3, &mut err);
            assert_eq!(err, 0);
            assert_eq!(TYPEOF(d3), SEXPTYPE::RAWSXP);
            assert_eq!(slice::from_raw_parts(RAW(d3), 8), &[0, 1, 2, 3, 4, 5, 6, 7]);
        }
    }

    #[test]
    fn test_r_decompress_rejects_truncated_input() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let raw = make_raw(&[1, 2, 3, 4]);
            let mut err: Rboolean = 0;
            assert_eq!(R_decompress1(raw, &mut err), R_NilValue());
            assert_eq!(err, 1);
            err = 0;
            assert_eq!(R_decompress2(raw, &mut err), R_NilValue());
            assert_eq!(err, 1);
            err = 0;
            assert_eq!(R_decompress3(raw, &mut err), R_NilValue());
            assert_eq!(err, 1);
        }
    }

    #[test]
    fn test_r_decompress2_3_support_legacy_markers() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let expected = [10, 20, 30, 40, 50, 60, 70, 80];
            let raw = make_raw(&expected);
            let c1 = R_compress1(raw);
            let c1_data = slice::from_raw_parts(RAW(c1), XLENGTH(c1) as usize);

            let mut marker1 = Vec::with_capacity(c1_data.len() + 1);
            marker1.extend_from_slice(&c1_data[..4]);
            marker1.push(b'1');
            marker1.extend_from_slice(&c1_data[4..]);
            let marker1_raw = make_raw(&marker1);

            let mut marker0 = Vec::with_capacity(expected.len() + 5);
            marker0.extend_from_slice(&swapped_len_bytes(expected.len()));
            marker0.push(b'0');
            marker0.extend_from_slice(&expected);
            let marker0_raw = make_raw(&marker0);

            let mut err: Rboolean = 0;
            let d21 = R_decompress2(marker1_raw, &mut err);
            assert_eq!(err, 0);
            assert_eq!(slice::from_raw_parts(RAW(d21), expected.len()), &expected);

            err = 0;
            let d31 = R_decompress3(marker1_raw, &mut err);
            assert_eq!(err, 0);
            assert_eq!(slice::from_raw_parts(RAW(d31), expected.len()), &expected);

            err = 0;
            let d20 = R_decompress2(marker0_raw, &mut err);
            assert_eq!(err, 0);
            assert_eq!(slice::from_raw_parts(RAW(d20), expected.len()), &expected);

            err = 0;
            let d30 = R_decompress3(marker0_raw, &mut err);
            assert_eq!(err, 0);
            assert_eq!(slice::from_raw_parts(RAW(d30), expected.len()), &expected);
        }
    }

    #[test]
    fn test_r_decompress2_3_reject_unknown_marker() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let mut blob = Vec::new();
            blob.extend_from_slice(&swapped_len_bytes(0));
            blob.push(b'X');
            let raw = make_raw(&blob);
            let mut err: Rboolean = 0;
            assert_eq!(R_decompress2(raw, &mut err), R_NilValue());
            assert_eq!(err, 1);
            err = 0;
            assert_eq!(R_decompress3(raw, &mut err), R_NilValue());
            assert_eq!(err, 1);
        }
    }

    #[test]
    fn test_r_write_connection_returns_zero() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let n = R_WriteConnection(ptr::null_mut(), ptr::null(), 42);
            assert_eq!(n, 0);
        }
    }

    // ---- Round-trip serialization tests ----

    #[test]
    fn test_roundtrip_integer_scalar() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s = Rf_ScalarInteger(42);
            let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            assert_ne!(raw, R_NilValue());
            assert!(XLENGTH(raw) > 0);

            let result = R_unserialize(raw, R_NilValue());
            assert_ne!(result, R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
            assert_eq!(LENGTH(result), 1);
            assert_eq!(*INTEGER(result), 42);
        }
    }

    #[test]
    fn test_roundtrip_real_scalar() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s = Rf_ScalarReal(3.14159);
            let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            assert_ne!(raw, R_NilValue());

            let result = R_unserialize(raw, R_NilValue());
            assert_ne!(result, R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP);
            assert_eq!(LENGTH(result), 1);
            assert!((*REAL(result) - 3.14159).abs() < 1e-10);
        }
    }

    #[test]
    fn test_roundtrip_logical_scalar() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s = Rf_ScalarLogical(1); // TRUE
            let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            assert_ne!(raw, R_NilValue());

            let result = R_unserialize(raw, R_NilValue());
            assert_ne!(result, R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::LGLSXP);
            assert_eq!(LENGTH(result), 1);
            assert_eq!(*LOGICAL(result), 1);
        }
    }

    #[test]
    fn test_roundtrip_integer_vector() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s = Rf_allocVector3(SEXPTYPE::INTSXP, 3);
            let data = INTEGER(s);
            *data = 10;
            *data.add(1) = 20;
            *data.add(2) = 30;

            let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            assert_ne!(raw, R_NilValue());

            let result = R_unserialize(raw, R_NilValue());
            assert_ne!(result, R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
            assert_eq!(LENGTH(result), 3);
            let rdata = INTEGER(result);
            assert_eq!(*rdata, 10);
            assert_eq!(*rdata.add(1), 20);
            assert_eq!(*rdata.add(2), 30);
        }
    }

    #[test]
    fn test_roundtrip_real_vector() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
            let data = REAL(s);
            *data = 1.5;
            *data.add(1) = 2.5;

            let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            assert_ne!(raw, R_NilValue());

            let result = R_unserialize(raw, R_NilValue());
            assert_ne!(result, R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP);
            assert_eq!(LENGTH(result), 2);
            let rdata = REAL(result);
            assert!((*rdata - 1.5).abs() < 1e-10);
            assert!((*rdata.add(1) - 2.5).abs() < 1e-10);
        }
    }

    #[test]
    fn test_roundtrip_complex_scalar() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s = Rf_ScalarComplex(Rcomplex { r: 1.0, i: 2.0 });
            let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            assert_ne!(raw, R_NilValue());

            let result = R_unserialize(raw, R_NilValue());
            assert_ne!(result, R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::CPLXSXP);
            assert_eq!(LENGTH(result), 1);
            let c = *COMPLEX(result);
            assert!((c.r - 1.0).abs() < 1e-10);
            assert!((c.i - 2.0).abs() < 1e-10);
        }
    }

    #[test]
    fn test_roundtrip_string_scalar() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s = Rf_mkString(c"hello".as_ptr());
            let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            assert_ne!(raw, R_NilValue());

            let result = R_unserialize(raw, R_NilValue());
            assert_ne!(result, R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::STRSXP);
            assert_eq!(LENGTH(result), 1);
            let elt = STRING_ELT(result, 0);
            assert!(!elt.is_null());
            // Check the string content
            let cstr = std::ffi::CStr::from_ptr(CHAR(elt));
            assert_eq!(cstr.to_bytes(), b"hello");
        }
    }

    #[test]
    fn test_roundtrip_string_vector() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
            let c1 = Rf_mkChar(c"foo".as_ptr());
            let c2 = Rf_mkChar(c"bar".as_ptr());
            SET_STRING_ELT(s, 0, c1);
            SET_STRING_ELT(s, 1, c2);

            let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            assert_ne!(raw, R_NilValue());

            let result = R_unserialize(raw, R_NilValue());
            assert_ne!(result, R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::STRSXP);
            assert_eq!(LENGTH(result), 2);
            let e1 = STRING_ELT(result, 0);
            let e2 = STRING_ELT(result, 1);
            let cstr1 = std::ffi::CStr::from_ptr(CHAR(e1));
            let cstr2 = std::ffi::CStr::from_ptr(CHAR(e2));
            assert_eq!(cstr1.to_bytes(), b"foo");
            assert_eq!(cstr2.to_bytes(), b"bar");
        }
    }

    #[test]
    fn test_roundtrip_raw_vector() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s = Rf_allocVector3(SEXPTYPE::RAWSXP, 3);
            let data = RAW(s);
            *data = 0xDE;
            *data.add(1) = 0xAD;
            *data.add(2) = 0xBE;

            let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            assert_ne!(raw, R_NilValue());

            let result = R_unserialize(raw, R_NilValue());
            assert_ne!(result, R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::RAWSXP);
            assert_eq!(LENGTH(result), 3);
            let rdata = RAW(result);
            assert_eq!(*rdata, 0xDE);
            assert_eq!(*rdata.add(1), 0xAD);
            assert_eq!(*rdata.add(2), 0xBE);
        }
    }

    #[test]
    fn test_roundtrip_empty_vector() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s = Rf_allocVector3(SEXPTYPE::INTSXP, 0);
            let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            assert_ne!(raw, R_NilValue());

            let result = R_unserialize(raw, R_NilValue());
            assert_ne!(result, R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
            assert_eq!(LENGTH(result), 0);
        }
    }

    #[test]
    fn test_roundtrip_null() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s = R_NilValue();
            let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            assert_ne!(raw, R_NilValue());

            let result = R_unserialize(raw, R_NilValue());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_roundtrip_na_string() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s = make_na_string_vector();
            let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            assert_ne!(raw, R_NilValue());

            let result = R_unserialize(raw, R_NilValue());
            assert_ne!(result, R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::STRSXP);
            assert_eq!(LENGTH(result), 1);
            assert_eq!(STRING_ELT(result, 0), R_NaString());
        }
    }

    #[test]
    fn test_roundtrip_special_singletons() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let mut values = vec![R_NilValue(), R_UnboundValue(), R_MissingArg()];
            for env in [R_GlobalEnv(), R_BaseEnv(), R_EmptyEnv()] {
                if !env.is_null() {
                    values.push(env);
                }
            }
            for value in values {
                let raw = R_serialize(
                    value,
                    R_NilValue(),
                    R_NilValue(),
                    R_NilValue(),
                    R_NilValue(),
                );
                assert_ne!(raw, R_NilValue());

                let result = R_unserialize(raw, R_NilValue());
                assert_eq!(result, value);
            }
        }
    }

    #[test]
    fn test_roundtrip_generic_list() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            // Create a list(c(1, 2), c(3.0, 4.0))
            let vec1 = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
            *INTEGER(vec1) = 1;
            *INTEGER(vec1).add(1) = 2;

            let vec2 = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
            *REAL(vec2) = 3.0;
            *REAL(vec2).add(1) = 4.0;

            let list = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
            SET_VECTOR_ELT(list, 0, vec1);
            SET_VECTOR_ELT(list, 1, vec2);

            let raw = R_serialize(list, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            assert_ne!(raw, R_NilValue());

            let result = R_unserialize(raw, R_NilValue());
            assert_ne!(result, R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::VECSXP);
            assert_eq!(LENGTH(result), 2);

            // Check first element (integer vector)
            let r1 = VECTOR_ELT(result, 0);
            assert_eq!(TYPEOF(r1), SEXPTYPE::INTSXP);
            assert_eq!(LENGTH(r1), 2);
            assert_eq!(*INTEGER(r1), 1);
            assert_eq!(*INTEGER(r1).add(1), 2);

            // Check second element (real vector)
            let r2 = VECTOR_ELT(result, 1);
            assert_eq!(TYPEOF(r2), SEXPTYPE::REALSXP);
            assert_eq!(LENGTH(r2), 2);
            assert!((*REAL(r2) - 3.0).abs() < 1e-10);
            assert!((*REAL(r2).add(1) - 4.0).abs() < 1e-10);
        }
    }

    #[test]
    fn test_roundtrip_nested_list() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            // Create list(list(1, 2), list(3, 4))
            let inner1 = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
            let inner2 = Rf_allocVector3(SEXPTYPE::VECSXP, 2);

            let v1 = Rf_ScalarInteger(1);
            let v2 = Rf_ScalarInteger(2);
            let v3 = Rf_ScalarInteger(3);
            let v4 = Rf_ScalarInteger(4);

            SET_VECTOR_ELT(inner1, 0, v1);
            SET_VECTOR_ELT(inner1, 1, v2);
            SET_VECTOR_ELT(inner2, 0, v3);
            SET_VECTOR_ELT(inner2, 1, v4);

            let outer = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
            SET_VECTOR_ELT(outer, 0, inner1);
            SET_VECTOR_ELT(outer, 1, inner2);

            let raw = R_serialize(
                outer,
                R_NilValue(),
                R_NilValue(),
                R_NilValue(),
                R_NilValue(),
            );
            assert_ne!(raw, R_NilValue());

            let result = R_unserialize(raw, R_NilValue());
            assert_ne!(result, R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::VECSXP);
            assert_eq!(LENGTH(result), 2);

            let r_inner1 = VECTOR_ELT(result, 0);
            assert_eq!(TYPEOF(r_inner1), SEXPTYPE::VECSXP);
            assert_eq!(LENGTH(r_inner1), 2);
            let rv1 = VECTOR_ELT(r_inner1, 0);
            let rv2 = VECTOR_ELT(r_inner1, 1);
            assert_eq!(TYPEOF(rv1), SEXPTYPE::INTSXP);
            assert_eq!(*INTEGER(rv1), 1);
            assert_eq!(TYPEOF(rv2), SEXPTYPE::INTSXP);
            assert_eq!(*INTEGER(rv2), 2);

            let r_inner2 = VECTOR_ELT(result, 1);
            assert_eq!(TYPEOF(r_inner2), SEXPTYPE::VECSXP);
            assert_eq!(LENGTH(r_inner2), 2);
            let rv3 = VECTOR_ELT(r_inner2, 0);
            let rv4 = VECTOR_ELT(r_inner2, 1);
            assert_eq!(TYPEOF(rv3), SEXPTYPE::INTSXP);
            assert_eq!(*INTEGER(rv3), 3);
            assert_eq!(TYPEOF(rv4), SEXPTYPE::INTSXP);
            assert_eq!(*INTEGER(rv4), 4);
        }
    }

    #[test]
    fn test_serialized_format_header() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s = Rf_ScalarInteger(1);
            let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            let len = XLENGTH(raw) as usize;
            let raw_data = RAW(raw);
            let data = slice::from_raw_parts(raw_data, len);

            // R's memory serialization defaults to XDR unless xdr = FALSE.
            assert_eq!(data[0], b'X');
            assert_eq!(data[1], b'\n');

            // Check version = 3
            let version = i32::from_be_bytes(data[2..6].try_into().unwrap_or([0; 4]));
            assert_eq!(version, 3);
        }
    }

    #[test]
    fn test_roundtrip_large_integer_vector() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let n: i32 = 100;
            let s = Rf_allocVector3(SEXPTYPE::INTSXP, n as R_xlen_t);
            let data = INTEGER(s);
            for i in 0..n as isize {
                *data.offset(i) = (i * i) as c_int;
            }

            let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            assert_ne!(raw, R_NilValue());

            let result = R_unserialize(raw, R_NilValue());
            assert_ne!(result, R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
            assert_eq!(LENGTH(result), n);
            let rdata = INTEGER(result);
            for i in 0..n as isize {
                assert_eq!(*rdata.offset(i), (i * i) as c_int);
            }
        }
    }

    #[test]
    fn test_binary_reader_errors() {
        let _session = crate::sexp::session::RSession::new();
        let data = [0u8; 3]; // Too short for an i32
        let mut reader = BinaryReader::new(&data);
        assert!(reader.read_i32().is_err());
        assert!(reader.read_f64().is_err());
        assert!(reader.read_bytes(10).is_err());
    }

    #[test]
    fn test_binary_reader_remaining() {
        let _session = crate::sexp::session::RSession::new();
        let data = [1u8, 2, 3, 4, 5];
        let mut reader = BinaryReader::new(&data);
        assert_eq!(reader.remaining(), 5);
        let _ = reader.read_byte();
        assert_eq!(reader.remaining(), 4);
    }
