use super::*;

// ---------------------------------------------------------------------------
// Lazy-load helpers (module-private)
// ---------------------------------------------------------------------------

pub unsafe fn CallHook(x: SEXP, fun: SEXP) -> SEXP {
    unsafe {
        let _fun_guard = protect(fun);
        let _x_guard = protect(x);
        // GNU serialize.c CallHook evaluates LANGSXP fun(x) in GlobalEnv.
        let call = Rf_lang2(fun, x);
        let _call_guard = protect(call);
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
        let owner =
            crate::sexp::owner::OwnerToken::current().unwrap_or_else(|e| error(&e.to_string()));
        let _pin = owner.pin().unwrap_or_else(|e| error(&e.to_string()));
        let operands = [file, key].map(|value| {
            owner
                .sexp(value)
                .and_then(crate::sexp::Sexp::into_owned)
                .unwrap_or_else(|e| error(&e.to_string()))
        });
        let path = sexp_to_path(file);
        owner
            .require_active()
            .unwrap_or_else(|e| error(&e.to_string()));
        if let Some(image) = crate::library::portable_package::database_path(&path) {
            let result = (|| {
                let owner =
                    crate::sexp::owner::OwnerToken::current().map_err(|error| error.to_string())?;
                let file = owner
                    .sexp(file)
                    .and_then(crate::sexp::object::Sexp::into_owned)
                    .map_err(|error| error.to_string())?;
                let key = owner
                    .sexp(key)
                    .and_then(crate::sexp::object::Sexp::into_owned)
                    .map_err(|error| error.to_string())?;
                image.read_database(file, key)
            })()
            .unwrap_or_else(|message| error(&message));
            return result
                .expect("exact portable methods database path")
                .as_raw();
        }
        if path == std::path::Path::new(crate::library::datasets::DATABASE)
            || path == std::path::Path::new(crate::library::datasets::PACKAGE_DATABASE)
        {
            let result = (|| {
                let owner = crate::sexp::owner::OwnerToken::current().map_err(|e| e.to_string())?;
                let file = owner
                    .sexp(file)
                    .and_then(crate::sexp::object::Sexp::into_owned)
                    .map_err(|e| e.to_string())?;
                let key = owner
                    .sexp(key)
                    .and_then(crate::sexp::object::Sexp::into_owned)
                    .map_err(|e| e.to_string())?;
                crate::library::datasets::read_database(file, key)
            })()
            .unwrap_or_else(|message| error(&message));
            return result.expect("exact virtual database path").as_raw();
        }
        if TYPEOF(key) != SEXPTYPE::INTSXP || LENGTH(key) != 2 {
            error("bad offset/length argument");
        }
        let offset = operands[1]
            .try_integer_elt(0)
            .unwrap_or_else(|e| error(&e.to_string()));
        let len = operands[1]
            .try_integer_elt(1)
            .unwrap_or_else(|e| error(&e.to_string()));
        owner
            .require_active()
            .unwrap_or_else(|e| error(&e.to_string()));
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
        let mut buf = super::owned_input::InputBytes::new(
            super::workspace::reserve(&owner, len as usize).unwrap_or_else(|e| error(&e)),
            len as usize,
        )
        .unwrap_or_else(|e| error(&e.to_string()));
        let mut chunk = [0; 8192];
        let mut remaining = len as usize;
        while remaining != 0 {
            let count = remaining.min(chunk.len());
            if fp.read_exact(&mut chunk[..count]).is_err() {
                error("read failed");
            }
            buf.write_all(&chunk[..count])
                .unwrap_or_else(|e| error(&e.to_string()));
            remaining -= count;
        }
        let value = raw_from_bytes(&buf);
        owner
            .require_active()
            .unwrap_or_else(|e| error(&e.to_string()));
        value
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
