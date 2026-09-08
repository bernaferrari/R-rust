use super::*;

// ---------------------------------------------------------------------------
// Lazy-load helpers (module-private)
// ---------------------------------------------------------------------------

pub unsafe fn CallHook(x: SEXP, fun: SEXP) -> SEXP {
    unsafe {
        let call = Rf_cons(fun, Rf_cons(x, R_NilValue()));
        Rf_eval(call, R_GlobalEnv())
    }
}

pub unsafe fn checkNotPromise(val: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(val) == SEXPTYPE::PROMSXP {
            error("cannot return a promise (PROMSXP) object");
        }
        val
    }
}

pub unsafe fn appendRawToFile(file: SEXP, bytes: SEXP) -> SEXP {
    unsafe {
        let path = sexp_to_path(file);
        if TYPEOF(bytes) != SEXPTYPE::RAWSXP {
            error("not a proper raw vector");
        }
        let len = XLENGTH(bytes) as usize;
        if len > i32::MAX as usize {
            error("write failed");
        }
        let mut fp = match OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
        {
            Ok(file) => file,
            Err(err) => error(&format!("cannot open file '{}': {}", path.display(), err)),
        };
        let pos = match fp.seek(SeekFrom::End(0)) {
            Ok(pos) => pos,
            Err(_) => error("write failed"),
        };
        let data = slice::from_raw_parts(RAW(bytes), len);
        if fp.write_all(data).is_err() || fp.flush().is_err() {
            error("write failed");
        }
        if pos > i32::MAX as u64 {
            error("write failed");
        }
        let key = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        if key.is_null() {
            error("write failed");
        }
        *INTEGER(key) = pos as c_int;
        *INTEGER(key).add(1) = len as c_int;
        key
    }
}

pub unsafe fn readRawFromFile(file: SEXP, key: SEXP) -> SEXP {
    unsafe {
        let path = sexp_to_path(file);
        if TYPEOF(key) != SEXPTYPE::INTSXP || LENGTH(key) != 2 {
            error("bad offset/length argument");
        }
        let offset = *INTEGER(key);
        let len = *INTEGER(key).add(1);
        if offset < 0 || len < 0 {
            error("bad offset/length argument");
        }

        let mut fp = match File::open(&path) {
            Ok(file) => file,
            Err(err) => error(&format!("cannot open file '{}': {}", path.display(), err)),
        };
        let filelen = match fp.seek(SeekFrom::End(0)) {
            Ok(pos) => pos,
            Err(_) => error("read failed"),
        };
        let offset_u64 = offset as u64;
        let len_u64 = len as u64;
        if offset_u64 > filelen || len_u64 > filelen.saturating_sub(offset_u64) {
            error("read failed");
        }
        if fp.seek(SeekFrom::Start(offset_u64)).is_err() {
            error("read failed");
        }
        let mut buf = vec![0u8; len as usize];
        if len > 0 && fp.read_exact(&mut buf).is_err() {
            error("read failed");
        }
        raw_from_bytes(&buf)
    }
}

pub unsafe fn R_lazyLoadDBinsertValue(
    value: SEXP,
    file: SEXP,
    ascii: SEXP,
    compsxp: SEXP,
    hook: SEXP,
) -> SEXP {
    unsafe {
        let mut data = R_serialize(value, R_NilValue(), ascii, R_NilValue(), hook);
        let compress = asInteger(compsxp);
        if compress == 3 {
            data = R_compress3(data);
        } else if compress == 2 {
            data = R_compress2(data);
        } else if compress != 0 {
            data = R_compress1(data);
        }
        appendRawToFile(file, data)
    }
}

pub unsafe fn R_getVarsFromFrame(vars: SEXP, env: SEXP, forcesxp: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(env) == SEXPTYPE::NILSXP {
            error("use of NULL environment is defunct");
        }
        if TYPEOF(env) != SEXPTYPE::ENVSXP {
            error("bad environment");
        }
        if TYPEOF(vars) != SEXPTYPE::STRSXP {
            error("bad variable names");
        }

        let force = asLogical(forcesxp);
        let len = LENGTH(vars);
        let val = Rf_allocVector3(SEXPTYPE::VECSXP, len as R_xlen_t);
        let _val_guard = protect(val);
        for i in 0..len {
            let name = STRING_ELT(vars, i as R_xlen_t);
            if name.is_null() {
                error("bad variable names");
            }
            let sym = Rf_install(CHAR(name));
            let mut tmp = R_findVarInFrame(env, sym);
            if tmp == R_UnboundValue() {
                error(&format!(
                    "object '{}' not found",
                    CStr::from_ptr(CHAR(name)).to_string_lossy()
                ));
            }
            if force != 0 && TYPEOF(tmp) == SEXPTYPE::PROMSXP {
                tmp = Rf_eval(tmp, R_GlobalEnv());
                if !tmp.is_null() {
                    SET_NAMED(tmp, 2);
                }
            }
            SET_VECTOR_ELT(val, i as R_xlen_t, tmp);
        }
        setAttrib(val, R_NamesSymbol(), vars);
        val
    }
}
