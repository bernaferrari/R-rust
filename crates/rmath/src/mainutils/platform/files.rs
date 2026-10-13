#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]
use super::*;

fn append_file(destination: &str, source: &str) -> bool {
    let Ok(metadata) = std::fs::metadata(source) else {
        return false;
    };
    if metadata.is_dir() {
        return false;
    }

    let Ok(mut input) = std::fs::File::open(source) else {
        return false;
    };
    let Ok(mut output) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(destination)
    else {
        return false;
    };

    std::io::copy(&mut input, &mut output).is_ok()
}

/// R's `file.append(file1, file2)` — append source files in `file2` to destinations in `file1`.
pub unsafe fn do_fileappend(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CADR, CAR, LENGTH, LOGICAL, TYPEOF};
        use crate::sexp::constructors::Rf_allocVector3;
        use crate::sexp::ffi::{FALSE, SEXPTYPE, TRUE};

        let file1 = CAR(args);
        let file2 = CADR(args);
        if TYPEOF(file1) != SEXPTYPE::STRSXP.as_c_int() {
            platform_error("invalid 'file1' argument");
        }
        if TYPEOF(file2) != SEXPTYPE::STRSXP.as_c_int() {
            platform_error("invalid 'file2' argument");
        }

        let n1 = LENGTH(file1) as usize;
        let n2 = LENGTH(file2) as usize;
        if n1 == 0 {
            platform_error("nothing to append to");
        }
        if n2 == 0 {
            return Rf_allocVector3(SEXPTYPE::LGLSXP.as_c_int(), 0);
        }
        let n = n1.max(n2);
        let ans = Rf_allocVector3(SEXPTYPE::LGLSXP.as_c_int(), n as crate::sexp::ffi::R_xlen_t);
        let _ans_guard = protect(ans);
        let out = LOGICAL(ans);

        for i in 0..n {
            let ok = match (path_at(file1, i % n1), path_at(file2, i % n2)) {
                (Some(destination), Some(source)) => append_file(&destination, &source),
                _ => false,
            };
            *out.add(i) = if ok { TRUE } else { FALSE };
        }
        ans
    }
}

/// R's `file.create()` — create file(s).
pub unsafe fn do_filecreate(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, LENGTH, STRING_ELT};
        use crate::sexp::constructors::Rf_allocVector3;
        use crate::sexp::ffi::{FALSE, SEXPTYPE, TRUE};
        use crate::sexp::globals::R_NilValue;

        let s = CAR(args);
        let ans = Rf_allocVector3(
            SEXPTYPE::LGLSXP.as_c_int(),
            LENGTH(s) as crate::sexp::ffi::R_xlen_t,
        );
        let _ans_guard = protect(ans);
        let pa = crate::sexp::accessors::LOGICAL(ans);

        for i in 0..LENGTH(s) as usize {
            let elt = STRING_ELT(s, i as crate::sexp::ffi::R_xlen_t);
            if elt.is_null() || elt == R_NilValue() {
                *pa.add(i) = crate::sexp::ffi::NA_INTEGER;
            } else {
                let c = CStr::from_ptr(crate::sexp::accessors::CHAR(elt));
                let path = c.to_str().unwrap_or("");
                *pa.add(i) = if create_file_with_session_umask(path).is_ok() {
                    TRUE
                } else {
                    FALSE
                };
            }
        }
        ans
    }
}

/// R's `file.remove()` — remove file(s).
pub unsafe fn do_fileremove(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, CDR, LENGTH, STRING_ELT};
        use crate::sexp::constructors::Rf_allocVector3;
        use crate::sexp::ffi::{FALSE, SEXPTYPE, TRUE};
        use crate::sexp::globals::R_NilValue;
        use std::fs;

        let mut paths = Vec::new();
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let s = CAR(current);
            if s.is_null() || s == R_NilValue() {
                paths.push(None);
            } else {
                for i in 0..LENGTH(s) as usize {
                    let elt = STRING_ELT(s, i as crate::sexp::ffi::R_xlen_t);
                    if elt.is_null() || elt == R_NilValue() {
                        paths.push(None);
                    } else {
                        let c = CStr::from_ptr(crate::sexp::accessors::CHAR(elt));
                        paths.push(Some(c.to_str().unwrap_or("").to_string()));
                    }
                }
            }
            current = CDR(current);
        }
        let ans = Rf_allocVector3(
            SEXPTYPE::LGLSXP.as_c_int(),
            paths.len() as crate::sexp::ffi::R_xlen_t,
        );
        let _ans_guard = protect(ans);
        let pa = crate::sexp::accessors::LOGICAL(ans);

        for (i, path) in paths.iter().enumerate() {
            if let Some(path) = path {
                *pa.add(i) = if fs::remove_file(path).is_ok() {
                    TRUE
                } else {
                    FALSE
                };
            } else {
                *pa.add(i) = crate::sexp::ffi::NA_INTEGER;
            }
        }
        ans
    }
}

/// R's `Sys.junction()` (Windows) / `file.link()` — create symbolic links.
#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn do_filesymlink(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CADR, CAR, LENGTH, STRING_ELT};
        use crate::sexp::constructors::Rf_allocVector3;
        use crate::sexp::ffi::{FALSE, SEXPTYPE, TRUE};
        use crate::sexp::globals::R_NilValue;
        use std::os::unix::fs::symlink;

        let from = CAR(args);
        let to = CADR(args);
        let n = LENGTH(from);
        let ans = Rf_allocVector3(SEXPTYPE::LGLSXP.as_c_int(), n as crate::sexp::ffi::R_xlen_t);
        let _ans_guard = protect(ans);
        let pa = crate::sexp::accessors::LOGICAL(ans);

        for i in 0..n as usize {
            let f = STRING_ELT(from, i as crate::sexp::ffi::R_xlen_t);
            let t = STRING_ELT(to, i as crate::sexp::ffi::R_xlen_t);
            if f.is_null() || t.is_null() || f == R_NilValue() || t == R_NilValue() {
                *pa.add(i) = crate::sexp::ffi::NA_INTEGER;
            } else {
                let fc = CStr::from_ptr(crate::sexp::accessors::CHAR(f))
                    .to_str()
                    .unwrap_or("");
                let tc = CStr::from_ptr(crate::sexp::accessors::CHAR(t))
                    .to_str()
                    .unwrap_or("");
                *pa.add(i) = if symlink(fc, tc).is_ok() { TRUE } else { FALSE };
            }
        }
        ans
    }
}

/// WASM M1 stub for `do_filesymlink`: `std::os::unix::fs::symlink`
/// does not exist on wasm32. Mirrors upstream R's no-symlink fallback
/// (platform.c `do_filesymlink` `#else` branch): warn that symbolic
/// links are unsupported on this platform and return an all-FALSE
/// logical vector of matching length (unsupported-operation path).
#[cfg(target_arch = "wasm32")]
pub unsafe fn do_filesymlink(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, LENGTH};
        use crate::sexp::constructors::Rf_allocVector3;
        use crate::sexp::ffi::{FALSE, SEXPTYPE};

        crate::mainutils::errors::Rf_warning(
            c"symbolic links are not supported on this platform".as_ptr(),
        );
        let n = LENGTH(CAR(args));
        let ans = Rf_allocVector3(SEXPTYPE::LGLSXP.as_c_int(), n as crate::sexp::ffi::R_xlen_t);
        let _ans_guard = protect(ans);
        let pa = crate::sexp::accessors::LOGICAL(ans);
        for i in 0..n as usize {
            *pa.add(i) = FALSE;
        }
        ans
    }
}

/// R's `file.link()` — create hard links.
pub unsafe fn do_filelink(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CADR, CAR, LENGTH, STRING_ELT};
        use crate::sexp::constructors::Rf_allocVector3;
        use crate::sexp::ffi::{FALSE, SEXPTYPE, TRUE};
        use crate::sexp::globals::R_NilValue;
        use std::fs::hard_link;

        let from = CAR(args);
        let to = CADR(args);
        let n = LENGTH(from);
        let ans = Rf_allocVector3(SEXPTYPE::LGLSXP.as_c_int(), n as crate::sexp::ffi::R_xlen_t);
        let _ans_guard = protect(ans);
        let pa = crate::sexp::accessors::LOGICAL(ans);

        for i in 0..n as usize {
            let f = STRING_ELT(from, i as crate::sexp::ffi::R_xlen_t);
            let t = STRING_ELT(to, i as crate::sexp::ffi::R_xlen_t);
            if f.is_null() || t.is_null() || f == R_NilValue() || t == R_NilValue() {
                *pa.add(i) = crate::sexp::ffi::NA_INTEGER;
            } else {
                let fc = CStr::from_ptr(crate::sexp::accessors::CHAR(f))
                    .to_str()
                    .unwrap_or("");
                let tc = CStr::from_ptr(crate::sexp::accessors::CHAR(t))
                    .to_str()
                    .unwrap_or("");
                *pa.add(i) = if hard_link(fc, tc).is_ok() {
                    TRUE
                } else {
                    FALSE
                };
            }
        }
        ans
    }
}

/// R's `file.rename()` — rename file(s).
pub unsafe fn do_filerename(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CADR, CAR, LENGTH, STRING_ELT};
        use crate::sexp::constructors::Rf_allocVector3;
        use crate::sexp::ffi::{FALSE, SEXPTYPE, TRUE};
        use crate::sexp::globals::R_NilValue;
        use std::fs;

        let from = CAR(args);
        let to = CADR(args);
        let n = LENGTH(from);
        let ans = Rf_allocVector3(SEXPTYPE::LGLSXP.as_c_int(), n as crate::sexp::ffi::R_xlen_t);
        let _ans_guard = protect(ans);
        let pa = crate::sexp::accessors::LOGICAL(ans);

        for i in 0..n as usize {
            let f = STRING_ELT(from, i as crate::sexp::ffi::R_xlen_t);
            let t = STRING_ELT(to, i as crate::sexp::ffi::R_xlen_t);
            if f.is_null() || t.is_null() || f == R_NilValue() || t == R_NilValue() {
                *pa.add(i) = crate::sexp::ffi::NA_INTEGER;
            } else {
                let fc = CStr::from_ptr(crate::sexp::accessors::CHAR(f))
                    .to_str()
                    .unwrap_or("");
                let tc = CStr::from_ptr(crate::sexp::accessors::CHAR(t))
                    .to_str()
                    .unwrap_or("");
                *pa.add(i) = if fs::rename(fc, tc).is_ok() {
                    TRUE
                } else {
                    FALSE
                };
            }
        }
        ans
    }
}

/// R's `file.info()` — get file information (size, mtime, etc.).
pub unsafe fn do_fileinfo(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{
            CADR, CAR, LENGTH, SET_STRING_ELT, SET_VECTOR_ELT, STRING_ELT,
        };
        use crate::sexp::constructors::{Rf_allocVector3, Rf_mkChar};
        use crate::sexp::ffi::{FALSE, NA_LOGICAL, SEXPTYPE, TRUE};
        use crate::sexp::globals::{R_NaString, R_NilValue};
        use std::fs;

        let files = CAR(args);
        let extra_arg = CADR(args);
        // GNU file.info's R wrapper defaults extra_cols to TRUE and the
        // internal rejects NA. This builtin binds `file.info` directly, so
        // both behaviors live here.
        #[cfg_attr(unix, allow(unused_mut))] // mutated only in the non-Unix extras block
        let mut extras = if extra_arg.is_null()
            || extra_arg == R_NilValue()
            || extra_arg == crate::sexp::globals::R_MissingArg()
        {
            true
        } else {
            let flag = crate::main::coerce::asLogical(extra_arg);
            if flag == NA_LOGICAL {
                crate::mainutils::essentials::base_error("invalid 'extra_cols' argument");
            }
            flag != 0
        };
        // The uid/gid/uname/grname block is Unix-only (GNU UNIX_EXTRAS);
        // there is never an `exe` column off Windows.
        #[cfg(not(unix))]
        {
            extras = false;
        }

        let n = LENGTH(files);
        let n_xlen = n as crate::sexp::ffi::R_xlen_t;
        let ncols: i32 = if extras { 10 } else { 6 };
        let mut guards = Vec::new();
        let ans = Rf_allocVector3(
            SEXPTYPE::VECSXP.as_c_int(),
            ncols as crate::sexp::ffi::R_xlen_t,
        );
        guards.push(protect(ans));

        let size_col = Rf_allocVector3(SEXPTYPE::REALSXP.as_c_int(), n_xlen);
        guards.push(protect(size_col));
        let isdir_col = Rf_allocVector3(SEXPTYPE::LGLSXP.as_c_int(), n_xlen);
        guards.push(protect(isdir_col));
        let mode_col = Rf_allocVector3(SEXPTYPE::INTSXP.as_c_int(), n_xlen);
        guards.push(protect(mode_col));
        let mode_class = Rf_allocVector3(SEXPTYPE::STRSXP.as_c_int(), 1);
        guards.push(protect(mode_class));
        SET_STRING_ELT(mode_class, 0, Rf_mkChar(c"octmode".as_ptr()));
        crate::eval::attrib_core::setAttrib(
            mode_col,
            crate::eval::attrib_core::R_ClassSymbol(),
            mode_class,
        );
        let mtime_col = Rf_allocVector3(SEXPTYPE::REALSXP.as_c_int(), n_xlen);
        guards.push(protect(mtime_col));
        let ctime_col = Rf_allocVector3(SEXPTYPE::REALSXP.as_c_int(), n_xlen);
        guards.push(protect(ctime_col));
        let atime_col = Rf_allocVector3(SEXPTYPE::REALSXP.as_c_int(), n_xlen);
        guards.push(protect(atime_col));
        #[cfg(unix)]
        let (uid_col, gid_col, uname_col, grname_col) = {
            let uid_col = Rf_allocVector3(SEXPTYPE::INTSXP.as_c_int(), n_xlen);
            guards.push(protect(uid_col));
            let gid_col = Rf_allocVector3(SEXPTYPE::INTSXP.as_c_int(), n_xlen);
            guards.push(protect(gid_col));
            let uname_col = Rf_allocVector3(SEXPTYPE::STRSXP.as_c_int(), n_xlen);
            guards.push(protect(uname_col));
            let grname_col = Rf_allocVector3(SEXPTYPE::STRSXP.as_c_int(), n_xlen);
            guards.push(protect(grname_col));
            (uid_col, gid_col, uname_col, grname_col)
        };

        // Fill one row's extras columns with NAs (used when a stat tier has
        // no owner information).
        #[cfg(unix)]
        macro_rules! extras_na {
            ($i:expr) => {{
                let i: usize = $i;
                *crate::sexp::accessors::INTEGER(uid_col).add(i) = crate::sexp::ffi::NA_INTEGER;
                *crate::sexp::accessors::INTEGER(gid_col).add(i) = crate::sexp::ffi::NA_INTEGER;
                SET_STRING_ELT(uname_col, i as crate::sexp::ffi::R_xlen_t, R_NaString());
                SET_STRING_ELT(grname_col, i as crate::sexp::ffi::R_xlen_t, R_NaString());
            }};
        }
        #[cfg(not(unix))]
        macro_rules! extras_na {
            ($i:expr) => {{
                let _ = $i;
            }};
        }

        for i in 0..n as usize {
            let elt = STRING_ELT(files, i as crate::sexp::ffi::R_xlen_t);
            if elt.is_null() || elt == R_NilValue() {
                *crate::sexp::accessors::REAL(size_col).add(i) = crate::sexp::ffi::NA_REAL;
                *crate::sexp::accessors::LOGICAL(isdir_col).add(i) = crate::sexp::ffi::NA_INTEGER;
                *crate::sexp::accessors::INTEGER(mode_col).add(i) = crate::sexp::ffi::NA_INTEGER;
                *crate::sexp::accessors::REAL(mtime_col).add(i) = crate::sexp::ffi::NA_REAL;
                *crate::sexp::accessors::REAL(ctime_col).add(i) = crate::sexp::ffi::NA_REAL;
                *crate::sexp::accessors::REAL(atime_col).add(i) = crate::sexp::ffi::NA_REAL;
                extras_na!(i);
                continue;
            }
            let c = CStr::from_ptr(crate::sexp::accessors::CHAR(elt));
            let path = c.to_str().unwrap_or("");
            if crate::library::datasets::path_exists(path) {
                let is_directory = crate::library::datasets::is_directory(path);
                *crate::sexp::accessors::REAL(size_col).add(i) =
                    crate::library::datasets::file_bytes(path)
                        .map_or(0.0, |bytes| bytes.len() as f64);
                *crate::sexp::accessors::LOGICAL(isdir_col).add(i) =
                    if is_directory { TRUE } else { FALSE };
                *crate::sexp::accessors::INTEGER(mode_col).add(i) =
                    if is_directory { 0o555 } else { 0o444 };
                *crate::sexp::accessors::REAL(mtime_col).add(i) = crate::sexp::ffi::NA_REAL;
                *crate::sexp::accessors::REAL(ctime_col).add(i) = crate::sexp::ffi::NA_REAL;
                *crate::sexp::accessors::REAL(atime_col).add(i) = crate::sexp::ffi::NA_REAL;
                extras_na!(i);
                continue;
            }
            if let Some(info) = crate::mainutils::browser_files::info_current(path) {
                *crate::sexp::accessors::REAL(size_col).add(i) = info.size as f64;
                *crate::sexp::accessors::LOGICAL(isdir_col).add(i) = FALSE;
                *crate::sexp::accessors::INTEGER(mode_col).add(i) = 0o644;
                *crate::sexp::accessors::REAL(mtime_col).add(i) = info.mtime;
                *crate::sexp::accessors::REAL(ctime_col).add(i) = info.ctime;
                *crate::sexp::accessors::REAL(atime_col).add(i) = info.atime;
                extras_na!(i);
                continue;
            }
            if crate::mainutils::browser_files::enabled() {
                *crate::sexp::accessors::REAL(size_col).add(i) = crate::sexp::ffi::NA_REAL;
                *crate::sexp::accessors::LOGICAL(isdir_col).add(i) = crate::sexp::ffi::NA_INTEGER;
                *crate::sexp::accessors::INTEGER(mode_col).add(i) = crate::sexp::ffi::NA_INTEGER;
                *crate::sexp::accessors::REAL(mtime_col).add(i) = crate::sexp::ffi::NA_REAL;
                *crate::sexp::accessors::REAL(ctime_col).add(i) = crate::sexp::ffi::NA_REAL;
                *crate::sexp::accessors::REAL(atime_col).add(i) = crate::sexp::ffi::NA_REAL;
                extras_na!(i);
                continue;
            }
            match fs::metadata(path) {
                Ok(meta) => {
                    *crate::sexp::accessors::REAL(size_col).add(i) = meta.len() as f64;
                    *crate::sexp::accessors::LOGICAL(isdir_col).add(i) =
                        if meta.is_dir() { TRUE } else { FALSE };
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::MetadataExt;
                        *crate::sexp::accessors::INTEGER(mode_col).add(i) =
                            (meta.mode() & 0o777) as c_int;
                        // GNU sums st_[acm]tim seconds and nanoseconds.
                        *crate::sexp::accessors::REAL(mtime_col).add(i) =
                            meta.mtime() as f64 + meta.mtime_nsec() as f64 / 1e9;
                        *crate::sexp::accessors::REAL(ctime_col).add(i) =
                            meta.ctime() as f64 + meta.ctime_nsec() as f64 / 1e9;
                        *crate::sexp::accessors::REAL(atime_col).add(i) =
                            meta.atime() as f64 + meta.atime_nsec() as f64 / 1e9;
                        let uid = meta.uid() as c_int;
                        let gid = meta.gid() as c_int;
                        *crate::sexp::accessors::INTEGER(uid_col).add(i) = uid;
                        *crate::sexp::accessors::INTEGER(gid_col).add(i) = gid;
                        let pw = libc::getpwuid(uid as libc::uid_t);
                        SET_STRING_ELT(
                            uname_col,
                            i as crate::sexp::ffi::R_xlen_t,
                            if pw.is_null() {
                                R_NaString()
                            } else {
                                Rf_mkChar((*pw).pw_name)
                            },
                        );
                        let gr = libc::getgrgid(gid as libc::gid_t);
                        SET_STRING_ELT(
                            grname_col,
                            i as crate::sexp::ffi::R_xlen_t,
                            if gr.is_null() {
                                R_NaString()
                            } else {
                                Rf_mkChar((*gr).gr_name)
                            },
                        );
                    }
                    #[cfg(not(unix))]
                    {
                        *crate::sexp::accessors::INTEGER(mode_col).add(i) = 0;
                        let dur = |t: Option<std::time::SystemTime>| {
                            t.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                                .map(|d| d.as_secs_f64())
                                .unwrap_or(crate::sexp::ffi::NA_REAL)
                        };
                        *crate::sexp::accessors::REAL(mtime_col).add(i) = dur(meta.modified().ok());
                        *crate::sexp::accessors::REAL(ctime_col).add(i) = dur(meta.created().ok());
                        *crate::sexp::accessors::REAL(atime_col).add(i) = dur(meta.accessed().ok());
                    }
                }
                Err(_) => {
                    *crate::sexp::accessors::REAL(size_col).add(i) = crate::sexp::ffi::NA_REAL;
                    *crate::sexp::accessors::LOGICAL(isdir_col).add(i) =
                        crate::sexp::ffi::NA_INTEGER;
                    *crate::sexp::accessors::INTEGER(mode_col).add(i) =
                        crate::sexp::ffi::NA_INTEGER;
                    *crate::sexp::accessors::REAL(mtime_col).add(i) = crate::sexp::ffi::NA_REAL;
                    *crate::sexp::accessors::REAL(ctime_col).add(i) = crate::sexp::ffi::NA_REAL;
                    *crate::sexp::accessors::REAL(atime_col).add(i) = crate::sexp::ffi::NA_REAL;
                    extras_na!(i);
                }
            }
        }

        SET_VECTOR_ELT(ans, 0, size_col);
        SET_VECTOR_ELT(ans, 1, isdir_col);
        SET_VECTOR_ELT(ans, 2, mode_col);
        SET_VECTOR_ELT(ans, 3, mtime_col);
        SET_VECTOR_ELT(ans, 4, ctime_col);
        SET_VECTOR_ELT(ans, 5, atime_col);
        #[cfg(unix)]
        if extras {
            SET_VECTOR_ELT(ans, 6, uid_col);
            SET_VECTOR_ELT(ans, 7, gid_col);
            SET_VECTOR_ELT(ans, 8, uname_col);
            SET_VECTOR_ELT(ans, 9, grname_col);
        }

        // GNU's R wrapper wraps the three time columns with .POSIXct():
        // class c("POSIXct", "POSIXt"), no tzone attribute.
        for col in [mtime_col, ctime_col, atime_col] {
            let posix_class = Rf_allocVector3(SEXPTYPE::STRSXP.as_c_int(), 2);
            guards.push(protect(posix_class));
            SET_STRING_ELT(posix_class, 0, Rf_mkChar(c"POSIXct".as_ptr()));
            SET_STRING_ELT(posix_class, 1, Rf_mkChar(c"POSIXt".as_ptr()));
            crate::eval::attrib_core::setAttrib(
                col,
                crate::eval::attrib_core::R_ClassSymbol(),
                posix_class,
            );
        }

        let name_lits: &[&std::ffi::CStr] = if extras {
            &[
                c"size", c"isdir", c"mode", c"mtime", c"ctime", c"atime", c"uid", c"gid", c"uname",
                c"grname",
            ]
        } else {
            &[c"size", c"isdir", c"mode", c"mtime", c"ctime", c"atime"]
        };
        let names = Rf_allocVector3(
            SEXPTYPE::STRSXP.as_c_int(),
            ncols as crate::sexp::ffi::R_xlen_t,
        );
        guards.push(protect(names));
        for (j, lit) in name_lits.iter().enumerate() {
            SET_STRING_ELT(
                names,
                j as crate::sexp::ffi::R_xlen_t,
                Rf_mkChar(lit.as_ptr()),
            );
        }
        crate::eval::attrib_core::setAttrib(ans, crate::eval::attrib_core::R_NamesSymbol(), names);

        // attr(res, "row.names") <- fn : character row names, no automatic
        // integer compression.
        let rn = Rf_allocVector3(SEXPTYPE::STRSXP.as_c_int(), n_xlen);
        guards.push(protect(rn));
        for i in 0..n as usize {
            let elt = STRING_ELT(files, i as crate::sexp::ffi::R_xlen_t);
            if elt.is_null() || elt == R_NilValue() {
                SET_STRING_ELT(
                    rn,
                    i as crate::sexp::ffi::R_xlen_t,
                    Rf_mkChar(b"NA\0".as_ptr() as *const _),
                );
            } else {
                SET_STRING_ELT(rn, i as crate::sexp::ffi::R_xlen_t, elt);
            }
        }
        crate::eval::attrib_core::setAttrib(ans, crate::eval::attrib_core::R_RowNamesSymbol(), rn);

        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        guards.push(protect(class));
        SET_STRING_ELT(class, 0, Rf_mkChar(b"data.frame\0".as_ptr() as *const _));
        crate::eval::attrib_core::setAttrib(ans, crate::eval::attrib_core::R_ClassSymbol(), class);

        ans
    }
}

pub unsafe fn do_setfiletime(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, CDR, LENGTH, LOGICAL, REAL, STRING_ELT};
        use crate::sexp::constructors::Rf_allocVector3;
        use crate::sexp::ffi::SEXPTYPE;

        let paths = CAR(args);
        let times = CAR(CDR(args));
        let n = LENGTH(paths);
        let ans = Rf_allocVector3(SEXPTYPE::LGLSXP.as_c_int(), n as crate::sexp::ffi::R_xlen_t);
        let _g = protect(ans);
        let nt = if times.is_null() { 0 } else { LENGTH(times) };
        for i in 0..n as usize {
            let elt = STRING_ELT(paths, i as crate::sexp::ffi::R_xlen_t);
            let secs = if nt == 0 {
                f64::NAN
            } else {
                *REAL(times).add(i % nt as usize)
            };
            let ok = if elt.is_null() || !secs.is_finite() {
                false
            } else {
                let path = CStr::from_ptr(crate::sexp::accessors::CHAR(elt))
                    .to_str()
                    .unwrap_or("");
                if crate::mainutils::browser_files::contains_current(path)
                    || crate::mainutils::browser_files::enabled()
                {
                    crate::mainutils::browser_files::set_time_current(path, secs)
                } else {
                    #[cfg(target_arch = "wasm32")]
                    {
                        false
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        let path = crate::sexp::accessors::CHAR(elt);
                        let whole = secs.trunc() as i64;
                        let nsec = ((secs - whole as f64) * 1e9) as i64;
                        if let (Some(tv_sec), Some(tv_nsec)) = (
                            libc::time_t::try_from(whole).ok(),
                            std::os::raw::c_long::try_from(nsec).ok(),
                        ) {
                            let ts = libc::timespec { tv_sec, tv_nsec };
                            let times_buf = [ts, ts];
                            libc::utimensat(libc::AT_FDCWD, path, times_buf.as_ptr(), 0) == 0
                        } else {
                            false
                        }
                    }
                }
            };
            *LOGICAL(ans).add(i) = if ok { 1 } else { 0 };
        }
        ans
    }
}

/// R's `file.size(...)` — return file sizes in bytes.
pub unsafe fn do_filesize(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, LENGTH, REAL, STRING_ELT};
        use crate::sexp::constructors::Rf_allocVector3;
        use crate::sexp::ffi::SEXPTYPE;
        use crate::sexp::globals::R_NilValue;

        let files = CAR(args);
        let n = LENGTH(files);
        let ans = Rf_allocVector3(
            SEXPTYPE::REALSXP.as_c_int(),
            n as crate::sexp::ffi::R_xlen_t,
        );
        let _ans_guard = protect(ans);
        let out = REAL(ans);
        for i in 0..n as usize {
            let elt = STRING_ELT(files, i as crate::sexp::ffi::R_xlen_t);
            *out.add(i) = if elt.is_null() || elt == R_NilValue() {
                crate::sexp::ffi::NA_REAL
            } else {
                let path = CStr::from_ptr(crate::sexp::accessors::CHAR(elt))
                    .to_str()
                    .unwrap_or("");
                if let Some(info) = crate::mainutils::browser_files::info_current(path) {
                    info.size as f64
                } else if crate::mainutils::browser_files::enabled() {
                    crate::sexp::ffi::NA_REAL
                } else {
                    std::fs::metadata(path)
                        .map(|meta| meta.len() as f64)
                        .unwrap_or(crate::sexp::ffi::NA_REAL)
                }
            };
        }
        ans
    }
}

/// R's `file.mtime(...)` — return modification times as POSIXct.
pub unsafe fn do_filemtime(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, LENGTH, REAL, SET_STRING_ELT, STRING_ELT};
        use crate::sexp::constructors::{Rf_allocVector3, Rf_mkChar};
        use crate::sexp::ffi::SEXPTYPE;
        use crate::sexp::globals::R_NilValue;

        let files = CAR(args);
        let n = LENGTH(files);
        let ans = Rf_allocVector3(
            SEXPTYPE::REALSXP.as_c_int(),
            n as crate::sexp::ffi::R_xlen_t,
        );
        let _ans_guard = protect(ans);
        let out = REAL(ans);
        for i in 0..n as usize {
            let elt = STRING_ELT(files, i as crate::sexp::ffi::R_xlen_t);
            *out.add(i) = if elt.is_null() || elt == R_NilValue() {
                crate::sexp::ffi::NA_REAL
            } else {
                let path = CStr::from_ptr(crate::sexp::accessors::CHAR(elt))
                    .to_str()
                    .unwrap_or("");
                if let Some(info) = crate::mainutils::browser_files::info_current(path) {
                    info.mtime
                } else if crate::mainutils::browser_files::enabled() {
                    crate::sexp::ffi::NA_REAL
                } else {
                    std::fs::metadata(path)
                        .ok()
                        .and_then(|meta| meta.modified().ok())
                        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|duration| {
                            duration.as_secs() as f64 + duration.subsec_nanos() as f64 / 1e9
                        })
                        .unwrap_or(crate::sexp::ffi::NA_REAL)
                }
            };
        }

        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        let _class_guard = protect(class);
        SET_STRING_ELT(class, 0, Rf_mkChar(b"POSIXct\0".as_ptr() as *const _));
        SET_STRING_ELT(class, 1, Rf_mkChar(b"POSIXt\0".as_ptr() as *const _));
        crate::eval::attrib_core::setAttrib(ans, crate::eval::attrib_core::R_ClassSymbol(), class);
        ans
    }
}

/// R's `dir.exists()` — check if directory exists.
pub unsafe fn do_direxists(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, LENGTH, STRING_ELT};
        use crate::sexp::constructors::Rf_allocVector3;
        use crate::sexp::ffi::{FALSE, SEXPTYPE, TRUE};
        use crate::sexp::globals::R_NilValue;
        use std::path::Path;

        let s = CAR(args);
        let ans = Rf_allocVector3(
            SEXPTYPE::LGLSXP.as_c_int(),
            LENGTH(s) as crate::sexp::ffi::R_xlen_t,
        );
        let _ans_guard = protect(ans);
        let pa = crate::sexp::accessors::LOGICAL(ans);

        for i in 0..LENGTH(s) as usize {
            let elt = STRING_ELT(s, i as crate::sexp::ffi::R_xlen_t);
            if elt.is_null() || elt == R_NilValue() {
                *pa.add(i) = crate::sexp::ffi::NA_INTEGER;
            } else {
                let c = CStr::from_ptr(crate::sexp::accessors::CHAR(elt));
                let path = c.to_str().unwrap_or("");
                *pa.add(i) = if crate::library::datasets::is_directory(path) {
                    TRUE
                } else if crate::mainutils::browser_files::enabled()
                    || crate::mainutils::browser_files::contains_current(path)
                {
                    FALSE
                } else if Path::new(path).is_dir() {
                    TRUE
                } else {
                    FALSE
                };
            }
        }
        ans
    }
}

/// R's `list.files()` — list files in directories.
pub unsafe fn do_listfiles(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, CDR, LENGTH, LOGICAL, SET_STRING_ELT, STRING_ELT};
        use crate::sexp::constructors::{Rf_allocVector3, Rf_mkChar};
        use crate::sexp::ffi::SEXPTYPE;
        use crate::sexp::globals::R_NilValue;

        fn logical_arg(value: SEXP, default: bool) -> bool {
            unsafe {
                if value.is_null() || value == R_NilValue() || LENGTH(value) == 0 {
                    return default;
                }
                let v = *LOGICAL(value);
                v != 0 && v != crate::sexp::ffi::NA_INTEGER
            }
        }

        fn push_dir_dots(base: &str, full_names: bool, entries: &mut Vec<String>) {
            if full_names {
                entries.push(format!("{}/.", base.trim_end_matches('/')));
                entries.push(format!("{}/..", base.trim_end_matches('/')));
            } else {
                entries.push(".".to_string());
                entries.push("..".to_string());
            }
        }

        #[derive(Clone, Copy)]
        struct ListFilesOptions<'a> {
            pattern: Option<&'a str>,
            all_files: bool,
            full_names: bool,
            recursive: bool,
            ignore_case: bool,
            include_dirs: bool,
        }

        fn pattern_matches(name: &str, options: ListFilesOptions<'_>) -> bool {
            options.pattern.is_none_or(|pattern| {
                crate::mainutils::grep::ere_is_match(pattern, name, options.ignore_case)
            })
        }

        // Keys are flat. Only the session root is a directory; any other path
        // is missing and must not be read from the host.
        fn collect_browser_list_files(
            path: &str,
            options: ListFilesOptions<'_>,
            entries: &mut Vec<String>,
        ) {
            if path != "." && !path.is_empty() {
                return;
            }
            for name in crate::mainutils::browser_files::list_current() {
                if !options.all_files && name.starts_with('.') {
                    continue;
                }
                if pattern_matches(&name, options) {
                    entries.push(name);
                }
            }
        }

        fn collect_list_files(
            base: &std::path::Path,
            relative: &std::path::Path,
            options: ListFilesOptions<'_>,
            entries: &mut Vec<String>,
        ) {
            let current = base.join(relative);
            let Ok(read_dir) = std::fs::read_dir(&current) else {
                return;
            };
            let mut children = Vec::new();
            for entry in read_dir.flatten() {
                children.push(entry);
            }
            children.sort_by_key(|a| a.file_name());

            for entry in children {
                let name = entry.file_name().to_string_lossy().to_string();
                if !options.all_files && name.starts_with('.') {
                    continue;
                }

                let child_rel = relative.join(&name);
                let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
                let display = child_rel.to_string_lossy().replace('\\', "/");
                let returned = if options.full_names {
                    base.join(&child_rel).to_string_lossy().to_string()
                } else {
                    display.clone()
                };

                if options.recursive {
                    if is_dir {
                        if options.include_dirs && pattern_matches(&display, options) {
                            entries.push(returned);
                        }
                        collect_list_files(base, &child_rel, options, entries);
                    } else if pattern_matches(&display, options) {
                        entries.push(returned);
                    }
                } else if pattern_matches(&name, options) {
                    entries.push(returned);
                }
            }
        }

        let mut paths = R_NilValue();
        let mut pattern = R_NilValue();
        let mut all_files = R_NilValue();
        let mut full_names = R_NilValue();
        let mut recursive = R_NilValue();
        let mut ignore_case = R_NilValue();
        let mut include_dirs = R_NilValue();
        let mut no_dotdot = R_NilValue();
        let mut positional = 0;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let value = CAR(current);
            match platform_tag_name(current).as_deref() {
                Some("path") => paths = value,
                Some("pattern") => pattern = value,
                Some("all.files") => all_files = value,
                Some("full.names") => full_names = value,
                Some("recursive") => recursive = value,
                Some("ignore.case") => ignore_case = value,
                Some("include.dirs") => include_dirs = value,
                Some("no..") => no_dotdot = value,
                Some(_) => {}
                None => {
                    match positional {
                        0 => paths = value,
                        1 => pattern = value,
                        2 => all_files = value,
                        3 => full_names = value,
                        4 => recursive = value,
                        5 => ignore_case = value,
                        6 => include_dirs = value,
                        7 => no_dotdot = value,
                        _ => {}
                    }
                    positional += 1;
                }
            }
            current = CDR(current);
        }

        let pattern_string = if pattern.is_null() || pattern == R_NilValue() || LENGTH(pattern) == 0
        {
            None
        } else {
            let elt = STRING_ELT(pattern, 0);
            if elt.is_null() || elt == R_NilValue() || elt == crate::sexp::globals::R_NaString() {
                Some(String::new())
            } else {
                Some(
                    CStr::from_ptr(crate::sexp::accessors::CHAR(elt))
                        .to_str()
                        .unwrap_or("")
                        .to_string(),
                )
            }
        };
        let options = ListFilesOptions {
            pattern: pattern_string.as_deref(),
            all_files: logical_arg(all_files, false),
            full_names: logical_arg(full_names, false),
            recursive: logical_arg(recursive, false),
            ignore_case: logical_arg(ignore_case, false),
            include_dirs: logical_arg(include_dirs, false),
        };
        let omit_dotdot = logical_arg(no_dotdot, false);

        let mut entries: Vec<String> = Vec::new();
        let browser_store = crate::mainutils::browser_files::enabled();
        let mut visit_path = |path: String| {
            if browser_store {
                collect_browser_list_files(&path, options, &mut entries);
                return;
            }
            if options.all_files && !omit_dotdot && !options.recursive && pattern_string.is_none() {
                push_dir_dots(&path, options.full_names, &mut entries);
            }
            collect_list_files(
                std::path::Path::new(&path),
                std::path::Path::new(""),
                options,
                &mut entries,
            );
        };

        if paths.is_null() || paths == R_NilValue() {
            visit_path(".".to_string());
        } else if LENGTH(paths) == 0 {
        } else {
            for i in 0..LENGTH(paths) as usize {
                if let Some(path) = path_at(paths, i) {
                    visit_path(path);
                }
            }
        }

        entries.sort();
        let ans = Rf_allocVector3(
            SEXPTYPE::STRSXP.as_c_int(),
            entries.len() as crate::sexp::ffi::R_xlen_t,
        );
        let _ans_guard = protect(ans);
        for (i, name) in entries.iter().enumerate() {
            SET_STRING_ELT(
                ans,
                i as crate::sexp::ffi::R_xlen_t,
                Rf_mkChar(CString::new(name.as_str()).unwrap_or_default().as_ptr()),
            );
        }
        ans
    }
}

/// R's `list.dirs()` — list directories.
pub unsafe fn do_listdirs(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, CDR, LENGTH, LOGICAL, SET_STRING_ELT};
        use crate::sexp::constructors::{Rf_allocVector3, Rf_mkChar};
        use crate::sexp::ffi::SEXPTYPE;
        use crate::sexp::globals::R_NilValue;

        fn logical_arg(value: SEXP, default: bool) -> bool {
            unsafe {
                if value.is_null() || value == R_NilValue() || LENGTH(value) == 0 {
                    return default;
                }
                let v = *LOGICAL(value);
                v != 0 && v != crate::sexp::ffi::NA_INTEGER
            }
        }

        fn collect_dirs(
            base: &std::path::Path,
            relative: &std::path::Path,
            full_names: bool,
            recursive: bool,
            include_self: bool,
            out: &mut Vec<String>,
        ) {
            if include_self {
                out.push(if full_names {
                    base.join(relative).to_string_lossy().to_string()
                } else {
                    relative.to_string_lossy().to_string()
                });
            }

            let current = base.join(relative);
            let Ok(read_dir) = std::fs::read_dir(&current) else {
                return;
            };
            let mut children = Vec::new();
            for entry in read_dir.flatten() {
                if entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
                    children.push(entry.file_name().to_string_lossy().to_string());
                }
            }
            children.sort();

            for child in children {
                let child_rel = relative.join(&child);
                if recursive {
                    collect_dirs(base, &child_rel, full_names, true, true, out);
                } else {
                    out.push(if full_names {
                        base.join(&child_rel).to_string_lossy().to_string()
                    } else {
                        child_rel.to_string_lossy().to_string()
                    });
                }
            }
        }

        let mut paths = R_NilValue();
        let mut full_names = R_NilValue();
        let mut recursive = R_NilValue();
        let mut positional = 0;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let value = CAR(current);
            match platform_tag_name(current).as_deref() {
                Some("path") => paths = value,
                Some("full.names") => full_names = value,
                Some("recursive") => recursive = value,
                Some(_) => {}
                None => {
                    match positional {
                        0 => paths = value,
                        1 => full_names = value,
                        2 => recursive = value,
                        _ => {}
                    }
                    positional += 1;
                }
            }
            current = CDR(current);
        }

        let do_full = logical_arg(full_names, true);
        let do_recursive = logical_arg(recursive, true);
        let mut entries: Vec<String> = Vec::new();

        if paths.is_null() || paths == R_NilValue() || LENGTH(paths) == 0 {
            collect_dirs(
                std::path::Path::new("."),
                std::path::Path::new(""),
                do_full,
                do_recursive,
                do_recursive,
                &mut entries,
            );
        } else {
            for i in 0..LENGTH(paths) as usize {
                if let Some(path) = path_at(paths, i) {
                    collect_dirs(
                        std::path::Path::new(&path),
                        std::path::Path::new(""),
                        do_full,
                        do_recursive,
                        do_recursive,
                        &mut entries,
                    );
                }
            }
        }

        let ans = Rf_allocVector3(
            SEXPTYPE::STRSXP.as_c_int(),
            entries.len() as crate::sexp::ffi::R_xlen_t,
        );
        let _ans_guard = protect(ans);
        for (i, name) in entries.iter().enumerate() {
            SET_STRING_ELT(
                ans,
                i as crate::sexp::ffi::R_xlen_t,
                Rf_mkChar(CString::new(name.as_str()).unwrap_or_default().as_ptr()),
            );
        }
        ans
    }
}

/// R's `R.home` — return R home directory.
///
/// Uses R_HOME environment variable, or falls back to the directory
/// containing the rmath library.
pub unsafe fn do_Rhome(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::constructors::Rf_mkString;
        use std::env;

        let home = env::var("R_HOME").unwrap_or_else(|_| discover_r_home());
        Rf_mkString(CString::new(home).unwrap_or_default().as_ptr())
    }
}

fn discover_r_home() -> String {
    if let Ok(out) = std::process::Command::new("R").arg("RHOME").output()
        && let Ok(text) = String::from_utf8(out.stdout)
    {
        let home = text.trim();
        if !home.is_empty() && std::path::Path::new(home).is_dir() {
            return home.to_string();
        }
    }
    "/usr/lib/R".to_string()
}

/// R's `file.exists()` — check if file exists.
pub unsafe fn do_fileexists(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, LENGTH, STRING_ELT};
        use crate::sexp::constructors::Rf_allocVector3;
        use crate::sexp::ffi::{FALSE, SEXPTYPE, TRUE};
        use crate::sexp::globals::R_NilValue;
        use std::path::Path;

        let s = CAR(args);
        let ans = Rf_allocVector3(
            SEXPTYPE::LGLSXP.as_c_int(),
            LENGTH(s) as crate::sexp::ffi::R_xlen_t,
        );
        let _ans_guard = protect(ans);
        let pa = crate::sexp::accessors::LOGICAL(ans);

        for i in 0..LENGTH(s) as usize {
            let elt = STRING_ELT(s, i as crate::sexp::ffi::R_xlen_t);
            if elt.is_null() || elt == R_NilValue() {
                *pa.add(i) = crate::sexp::ffi::NA_INTEGER;
            } else {
                let c = CStr::from_ptr(crate::sexp::accessors::CHAR(elt));
                let path = c.to_str().unwrap_or("");
                *pa.add(i) = if crate::library::datasets::path_exists(path)
                    || crate::library::builtin_files::path_exists(path)
                    || crate::mainutils::browser_files::contains_current(path)
                {
                    TRUE
                } else if crate::mainutils::browser_files::enabled() {
                    FALSE
                } else if Path::new(path).exists() {
                    TRUE
                } else {
                    FALSE
                };
            }
        }
        ans
    }
}

/// R's `file.choose()` — interactive file chooser.
/// Raises the same cancellation error GNU R reports when no chooser supplies a path.
pub unsafe fn do_filechoose(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    platform_error("file choice cancelled")
}

/// R's `file.access()` — check file access permissions.
pub unsafe fn do_fileaccess(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CADR, CAR, INTEGER, LENGTH, STRING_ELT};
        use crate::sexp::constructors::Rf_allocVector3;
        use crate::sexp::ffi::SEXPTYPE;
        use crate::sexp::globals::R_NilValue;
        use std::path::Path;

        let files = CAR(args);
        let mode_arg = CADR(args);
        let n = LENGTH(files);

        // mode: 0=exists, 1=executable, 2=writable, 4=readable
        let mut mode = 0i32;
        if !mode_arg.is_null() && mode_arg != R_NilValue() {
            mode = crate::mainutils::coerce::asInteger(mode_arg);
        }

        let ans = Rf_allocVector3(SEXPTYPE::INTSXP.as_c_int(), n as crate::sexp::ffi::R_xlen_t);
        let _ans_guard = protect(ans);
        let pa = INTEGER(ans);

        for i in 0..n as usize {
            let elt = STRING_ELT(files, i as crate::sexp::ffi::R_xlen_t);
            if elt.is_null() || elt == R_NilValue() || elt == crate::sexp::globals::R_NaString() {
                *pa.add(i) = -1;
            } else {
                let c = CStr::from_ptr(crate::sexp::accessors::CHAR(elt));
                let path = c.to_str().unwrap_or("");
                let p = Path::new(path);
                let allowed = if crate::mainutils::browser_files::contains_current(path) {
                    mode & 1 == 0
                } else if crate::mainutils::browser_files::enabled() {
                    false
                } else if mode == 0 {
                    p.exists()
                } else {
                    std::fs::metadata(path)
                        .map(|m| {
                            let mut ok = true;
                            if mode & 2 != 0 {
                                ok &= !m.permissions().readonly();
                            }
                            if mode & 1 != 0 {
                                #[cfg(unix)]
                                {
                                    ok &= std::os::unix::fs::PermissionsExt::mode(&m.permissions())
                                        & 0o111
                                        != 0;
                                }
                                #[cfg(not(unix))]
                                {
                                    ok = false;
                                }
                            }
                            ok
                        })
                        .unwrap_or(false)
                };

                *pa.add(i) = if allowed { 0 } else { -1 };
            }
        }
        crate::eval::attrib_core::setAttrib(ans, crate::eval::attrib_core::R_NamesSymbol(), files);
        ans
    }
}

/// Remove flat store keys. Missing paths are FALSE and are not looked up on
/// the host, even when `recursive` is TRUE.
unsafe fn unlink_browser_files(args: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, LENGTH, LOGICAL};
        use crate::sexp::constructors::Rf_allocVector3;
        use crate::sexp::ffi::{FALSE, SEXPTYPE, TRUE};
        use crate::sexp::globals::R_NilValue;

        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::LGLSXP.as_c_int(), 0);
        }
        let n = LENGTH(x);
        let ans = Rf_allocVector3(SEXPTYPE::LGLSXP.as_c_int(), n as crate::sexp::ffi::R_xlen_t);
        let _guard = protect(ans);
        let out = LOGICAL(ans);
        for i in 0..n as usize {
            let removed = match path_at(x, i) {
                Some(path) => crate::mainutils::browser_files::remove_current(&path),
                None => false,
            };
            *out.add(i) = if removed { TRUE } else { FALSE };
        }
        ans
    }
}

/// R's `unlink()` — remove files or directories.
pub unsafe fn do_unlink(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CADDR, CADR, CAR, LENGTH, LOGICAL, STRING_ELT};
        use crate::sexp::constructors::Rf_ScalarInteger;
        use crate::sexp::globals::R_NilValue;

        if crate::mainutils::browser_files::enabled() {
            return unlink_browser_files(args);
        }

        let x = CAR(args);
        let recursive = CADR(args);
        let force = CADDR(args);

        let do_recursive = if !recursive.is_null() && recursive != R_NilValue() {
            let v = *LOGICAL(recursive);
            v != 0 && v != crate::sexp::ffi::NA_INTEGER
        } else {
            false
        };

        let n = LENGTH(x);
        let mut success = 0i32;
        for i in 0..n as usize {
            let elt = STRING_ELT(x, i as crate::sexp::ffi::R_xlen_t);
            if elt.is_null() || elt == R_NilValue() {
                continue;
            }
            let c = CStr::from_ptr(crate::sexp::accessors::CHAR(elt));
            let path = c.to_str().unwrap_or("");
            let result = if do_recursive {
                std::fs::remove_dir_all(path)
            } else {
                std::fs::remove_file(path).or_else(|_| std::fs::remove_dir(path))
            };
            if result.is_ok() {
                success += 1;
            }
        }
        Rf_ScalarInteger(success)
    }
}

/// R's `Sys.getlocale()` — get locale category.
pub unsafe fn do_getlocale(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, INTEGER};
        use crate::sexp::constructors::Rf_mkString;
        use crate::sexp::globals::R_NilValue;

        let category = CAR(args);
        let locale = if !category.is_null() && category != R_NilValue() {
            match *INTEGER(category) {
                1 => "LC_ALL",
                2 => "LC_COLLATE",
                3 => "LC_CTYPE",
                4 => "LC_MONETARY",
                5 => "LC_NUMERIC",
                6 => "LC_TIME",
                7 => "LC_MESSAGES",
                _ => "LC_ALL",
            }
        } else {
            "LC_ALL"
        };

        // Use std::env for locale info
        let val = match locale {
            "LC_ALL" => std::env::var("LC_ALL")
                .unwrap_or_else(|_| std::env::var("LANG").unwrap_or_else(|_| "C".to_string())),
            "LC_COLLATE" => std::env::var("LC_COLLATE").unwrap_or_else(|_| String::new()),
            "LC_CTYPE" => std::env::var("LC_CTYPE").unwrap_or_else(|_| String::new()),
            "LC_MONETARY" => std::env::var("LC_MONETARY").unwrap_or_else(|_| String::new()),
            "LC_NUMERIC" => std::env::var("LC_NUMERIC").unwrap_or_else(|_| String::new()),
            "LC_TIME" => std::env::var("LC_TIME").unwrap_or_else(|_| String::new()),
            "LC_MESSAGES" => std::env::var("LC_MESSAGES").unwrap_or_else(|_| String::new()),
            _ => String::new(),
        };

        Rf_mkString(CString::new(val).unwrap_or_default().as_ptr())
    }
}

/// R's `Sys.setlocale()` — set locale.
pub unsafe fn do_setlocale(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CADR, CAR, CHAR, INTEGER, STRING_ELT};
        use crate::sexp::constructors::Rf_mkString;
        use crate::sexp::globals::R_NilValue;
        use std::ffi::CStr;

        let category = CAR(args);
        let locale = CADR(args);

        let cat_name = if !category.is_null() && category != R_NilValue() {
            match *INTEGER(category) {
                1 => "LC_ALL",
                2 => "LC_COLLATE",
                3 => "LC_CTYPE",
                4 => "LC_MONETARY",
                5 => "LC_NUMERIC",
                6 => "LC_TIME",
                7 => "LC_MESSAGES",
                _ => "LC_ALL",
            }
        } else {
            "LC_ALL"
        };

        let loc_str = if !locale.is_null() && locale != R_NilValue() {
            let elt = STRING_ELT(locale, 0);
            if !elt.is_null() && elt != R_NilValue() {
                CStr::from_ptr(CHAR(elt)).to_str().unwrap_or("").to_string()
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        let allowed = crate::sexp::instance::with_required_current_instance(|inst| {
            (*inst).eval_state.capabilities.allow_environment_mutation
        });
        if !allowed {
            // Queries use Sys.getlocale; every Sys.setlocale call mutates
            // or selects process-global locale state.
            return Rf_mkString(b"\0".as_ptr() as *const _);
        }

        // Attempt to set locale via libc
        #[cfg(not(target_arch = "wasm32"))]
        let result = libc::setlocale(
            match cat_name {
                "LC_ALL" => libc::LC_ALL,
                "LC_COLLATE" => libc::LC_COLLATE,
                "LC_CTYPE" => libc::LC_CTYPE,
                "LC_MONETARY" => libc::LC_MONETARY,
                "LC_NUMERIC" => libc::LC_NUMERIC,
                "LC_TIME" => libc::LC_TIME,
                "LC_MESSAGES" => libc::LC_MESSAGES,
                _ => libc::LC_ALL,
            },
            if loc_str.is_empty() {
                std::ptr::null()
            } else {
                loc_str.as_ptr() as *const _
            },
        );
        // wasm32: no locale subsystem in the sandbox; the "C" locale is
        // always in effect and every set request reports "C".
        #[cfg(target_arch = "wasm32")]
        let result: *const core::ffi::c_char = {
            let _ = (cat_name, loc_str);
            c"C".as_ptr()
        };

        if result.is_null() {
            Rf_mkString(b"\0".as_ptr() as *const _)
        } else {
            let s = CStr::from_ptr(result);
            Rf_mkString(
                CString::new(s.to_str().unwrap_or(""))
                    .unwrap_or_default()
                    .as_ptr(),
            )
        }
    }
}

/// R's `Sys.localeconv()` — locale conventions.
pub unsafe fn do_localeconv(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::SET_STRING_ELT;
        use crate::sexp::constructors::{Rf_allocVector3, Rf_mkChar};
        use crate::sexp::ffi::SEXPTYPE;

        #[cfg(not(target_arch = "wasm32"))]
        let lc = libc::localeconv();
        let ans = Rf_allocVector3(SEXPTYPE::STRSXP, 7);
        let _ans_guard = protect(ans);
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, 7);
        let _names_guard = protect(names);

        let field_names = [
            "decimal_point",
            "thousands_sep",
            "int_curr_symbol",
            "currency_symbol",
            "mon_decimal_point",
            "positive_sign",
            "negative_sign",
        ];
        #[cfg(not(target_arch = "wasm32"))]
        let field_ptrs: [*const c_char; 7] = [
            (*lc).decimal_point,
            (*lc).thousands_sep,
            (*lc).int_curr_symbol,
            (*lc).currency_symbol,
            (*lc).mon_decimal_point,
            (*lc).positive_sign,
            (*lc).negative_sign,
        ];
        // wasm32: no locale subsystem; report the C-locale conventions —
        // the same values do_Sys_localeconv() documents as the portable
        // fallback ("." decimal point, empty separators/signs/symbols).
        #[cfg(target_arch = "wasm32")]
        let field_ptrs: [*const c_char; 7] = [
            c".".as_ptr(),
            c"".as_ptr(),
            c"".as_ptr(),
            c"".as_ptr(),
            c"".as_ptr(),
            c"".as_ptr(),
            c"".as_ptr(),
        ];

        for (i, (name, val)) in field_names.iter().zip(field_ptrs.iter()).enumerate() {
            SET_STRING_ELT(
                names,
                i as crate::sexp::ffi::R_xlen_t,
                Rf_mkChar(CString::new(*name).unwrap_or_default().as_ptr()),
            );
            if !val.is_null() {
                let s = CStr::from_ptr(*val);
                SET_STRING_ELT(
                    ans,
                    i as crate::sexp::ffi::R_xlen_t,
                    Rf_mkChar(
                        CString::new(s.to_str().unwrap_or(""))
                            .unwrap_or_default()
                            .as_ptr(),
                    ),
                );
            } else {
                SET_STRING_ELT(
                    ans,
                    i as crate::sexp::ffi::R_xlen_t,
                    Rf_mkChar(b"\0".as_ptr() as *const _),
                );
            }
        }

        crate::eval::attrib_core::setAttrib(ans, crate::eval::attrib_core::R_NamesSymbol(), names);
        ans
    }
}
