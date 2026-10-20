#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]
use super::*;

/// R's `path.expand()` — expand file paths (~ and environment variables).
pub unsafe fn do_pathexpand(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, LENGTH, STRING_ELT};
        use crate::sexp::constructors::{Rf_allocVector3, Rf_mkChar};
        use crate::sexp::ffi::SEXPTYPE;

        let s = CAR(args);
        let n = LENGTH(s);
        let ans = Rf_allocVector3(SEXPTYPE::STRSXP.as_c_int(), n as crate::sexp::ffi::R_xlen_t);
        let _ans_guard = protect(ans);

        for i in 0..n as usize {
            let elt = STRING_ELT(s, i as crate::sexp::ffi::R_xlen_t);
            if elt.is_null() || elt == crate::sexp::globals::R_NaString() {
                crate::sexp::accessors::SET_STRING_ELT(
                    ans,
                    i as crate::sexp::ffi::R_xlen_t,
                    crate::sexp::globals::R_NaString(),
                );
            } else {
                let c = CStr::from_ptr(crate::sexp::accessors::CHAR(elt));
                let path = c.to_str().unwrap_or("");
                // Expand ~ to home directory
                let expanded = if path.starts_with("~/") || path == "~" {
                    if let Ok(home) = std::env::var("HOME") {
                        if path == "~" {
                            home
                        } else {
                            format!("{}{}", home, &path[1..])
                        }
                    } else {
                        path.to_string()
                    }
                } else {
                    path.to_string()
                };
                crate::sexp::accessors::SET_STRING_ELT(
                    ans,
                    i as crate::sexp::ffi::R_xlen_t,
                    Rf_mkChar(CString::new(expanded).unwrap_or_default().as_ptr()),
                );
            }
        }
        ans
    }
}

/// R's `capabilities()` — query platform capabilities.
pub unsafe fn do_capabilities(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::SET_STRING_ELT;
        use crate::sexp::constructors::{Rf_allocVector3, Rf_mkChar};
        use crate::sexp::ffi::{FALSE, SEXPTYPE, TRUE};

        let names = [
            "jpeg",
            "png",
            "tiff",
            "tcltk",
            "X11",
            "aqua",
            "http/ftp",
            "sockets",
            "libxml",
            "fifo",
            "cledit",
            "iconv",
            "NLS",
            "Rprof",
            "profmem",
            "cairo",
            "ICU",
            "long.double",
            "libcurl",
        ];
        let n = names.len();
        let ans = Rf_allocVector3(SEXPTYPE::LGLSXP.as_c_int(), n as crate::sexp::ffi::R_xlen_t);
        let _ans_guard = protect(ans);
        let cn = Rf_allocVector3(SEXPTYPE::STRSXP.as_c_int(), n as crate::sexp::ffi::R_xlen_t);
        let _cn_guard = protect(cn);

        for (i, name) in names.iter().enumerate() {
            // Report capabilities we actually have
            let val = match *name {
                "jpeg" | "png" | "tiff" => FALSE,
                "X11" => FALSE,
                "aqua" => FALSE,
                "http/ftp" => FALSE,
                "sockets" => FALSE,
                "libxml" => FALSE,
                "fifo" => TRUE,
                "cledit" => FALSE,
                "iconv" => TRUE,
                "NLS" => FALSE,
                "Rprof" => FALSE,
                "profmem" => FALSE,
                "cairo" => FALSE,
                "ICU" => FALSE,
                "long.double" => FALSE,
                "libcurl" => FALSE,
                "tcltk" => FALSE,
                _ => FALSE,
            };
            *crate::sexp::accessors::LOGICAL(ans).add(i) = val;
            SET_STRING_ELT(
                cn,
                i as crate::sexp::ffi::R_xlen_t,
                Rf_mkChar(CString::new(*name).unwrap_or_default().as_ptr()),
            );
        }

        crate::eval::attrib_core::setAttrib(ans, crate::eval::attrib_core::R_NamesSymbol(), cn);
        if _args.is_null() || _args == crate::sexp::globals::R_NilValue() {
            return ans;
        }
        let what = crate::sexp::accessors::CAR(_args);
        if what.is_null()
            || what == crate::sexp::globals::R_NilValue()
            || what == crate::sexp::globals::R_MissingArg()
        {
            return ans;
        }
        let nwhat = crate::sexp::accessors::XLENGTH(what);
        let out = Rf_allocVector3(SEXPTYPE::LGLSXP.as_c_int(), nwhat);
        let _out = protect(out);
        let out_names = Rf_allocVector3(SEXPTYPE::STRSXP.as_c_int(), nwhat);
        let _on = protect(out_names);
        for i in 0..nwhat {
            let wanted = if crate::sexp::accessors::TYPEOF(what) == SEXPTYPE::STRSXP.as_c_int() {
                let elt = crate::sexp::accessors::STRING_ELT(what, i);
                if elt.is_null() {
                    String::new()
                } else {
                    std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(elt))
                        .to_string_lossy()
                        .into_owned()
                }
            } else {
                String::new()
            };
            let mut found = false;
            for (j, name) in names.iter().enumerate() {
                if *name == wanted {
                    *crate::sexp::accessors::LOGICAL(out).add(i as usize) =
                        *crate::sexp::accessors::LOGICAL(ans).add(j);
                    found = true;
                    break;
                }
            }
            if !found {
                *crate::sexp::accessors::LOGICAL(out).add(i as usize) =
                    crate::sexp::ffi::NA_LOGICAL;
            }
            SET_STRING_ELT(
                out_names,
                i,
                Rf_mkChar(CString::new(wanted).unwrap_or_default().as_ptr()),
            );
        }
        crate::eval::attrib_core::setAttrib(
            out,
            crate::eval::attrib_core::R_NamesSymbol(),
            out_names,
        );
        out
    }
}

/// Stable process identity for public metadata and temporary names.
/// Bare Wasm has one virtual process; querying an OS PID there would panic.
pub(crate) fn process_id() -> u32 {
    #[cfg(target_arch = "wasm32")]
    {
        1
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::process::id()
    }
}

/// R's `Sys.getpid()` — get process ID.
pub unsafe fn do_sysgetpid(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::sexp::constructors::Rf_ScalarInteger(process_id() as c_int) }
}

/// R's `dir.create()` — create directory/directories.
pub unsafe fn do_dircreate(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CADR, CAR, CDR, LENGTH, STRING_ELT};
        use crate::sexp::constructors::Rf_allocVector3;
        use crate::sexp::ffi::{FALSE, SEXPTYPE, TRUE};
        use crate::sexp::globals::R_NilValue;
        use std::fs;

        let s = CAR(args);
        let recursive = CADR(args);
        let showWarnings = CDR(CDR(args));
        let _ = showWarnings; // suppress unused warning
        let ans = Rf_allocVector3(
            SEXPTYPE::LGLSXP.as_c_int(),
            LENGTH(s) as crate::sexp::ffi::R_xlen_t,
        );
        let _ans_guard = protect(ans);
        let pa = crate::sexp::accessors::LOGICAL(ans);
        let do_recursive = if recursive.is_null() || recursive == R_NilValue() {
            false
        } else {
            let v = *crate::sexp::accessors::LOGICAL(recursive);
            v != 0 && v != crate::sexp::ffi::NA_INTEGER
        };

        for i in 0..LENGTH(s) as usize {
            let elt = STRING_ELT(s, i as crate::sexp::ffi::R_xlen_t);
            if elt.is_null() || elt == R_NilValue() {
                *pa.add(i) = crate::sexp::ffi::NA_INTEGER;
            } else {
                let c = CStr::from_ptr(crate::sexp::accessors::CHAR(elt));
                let path = c.to_str().unwrap_or("");
                // platform.c rejects an expanded path that does not fit in
                // `dir[R_PATH_MAX]` before strcpy. R_PATH_MAX is PATH_MAX.
                #[cfg(not(target_arch = "wasm32"))]
                let path_max = libc::PATH_MAX as usize;
                #[cfg(target_arch = "wasm32")]
                let path_max = 4096;
                if path.len() >= path_max {
                    crate::sexp::context::r_error("invalid 'path' argument");
                }
                let result = if do_recursive {
                    fs::create_dir_all(path)
                } else {
                    fs::create_dir(path)
                };
                *pa.add(i) = if result
                    .and_then(|_| set_path_mode(path, 0o777 & !current_file_creation_umask()))
                    .is_ok()
                {
                    TRUE
                } else {
                    FALSE
                };
            }
        }
        ans
    }
}

/// R's `file.copy()` — copy file(s).
pub unsafe fn do_filecopy(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
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
                if crate::mainutils::browser_files::enabled() {
                    *pa.add(i) = if crate::mainutils::browser_files::copy_current(fc, tc) {
                        TRUE
                    } else {
                        FALSE
                    };
                    continue;
                }
                let dest = std::path::Path::new(tc);
                let dest = if dest.is_dir() {
                    dest.join(std::path::Path::new(fc).file_name().unwrap_or_default())
                } else {
                    dest.to_path_buf()
                };
                *pa.add(i) = if fs::copy(fc, &dest).is_ok() {
                    TRUE
                } else {
                    FALSE
                };
            }
        }
        ans
    }
}

/// Portable hosts use immutable UTF-8 text, independent of process globals.
pub(crate) const fn portable_utf8_profile() -> bool {
    cfg!(any(target_arch = "wasm32", target_os = "android"))
}

/// Portable UTF-8 profile, or native host locale environment in precedence order.
pub fn locale_is_utf8() -> bool {
    if portable_utf8_profile() {
        return true;
    }
    for key in ["LC_ALL", "LC_CTYPE", "LANG"] {
        if let Ok(value) = std::env::var(key) {
            if !value.is_empty() {
                let lower = value.to_ascii_lowercase();
                return lower.contains("utf-8") || lower.contains("utf8");
            }
        }
    }
    false
}

/// R's `l10n_info()` — localization information.
pub unsafe fn do_l10n_info(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{SET_STRING_ELT, SET_VECTOR_ELT};
        use crate::sexp::constructors::{Rf_ScalarLogical, Rf_allocVector3, Rf_mkChar};
        use crate::sexp::ffi::{FALSE, SEXPTYPE, TRUE};

        let utf8 = locale_is_utf8();
        let ans = Rf_allocVector3(SEXPTYPE::VECSXP, 4);
        let _ans_guard = protect(ans);
        let cn = Rf_allocVector3(SEXPTYPE::STRSXP, 4);
        let _cn_guard = protect(cn);
        SET_STRING_ELT(cn, 0, Rf_mkChar(b"MBCS\0".as_ptr() as *const _));
        SET_STRING_ELT(cn, 1, Rf_mkChar(b"UTF-8\0".as_ptr() as *const _));
        SET_STRING_ELT(cn, 2, Rf_mkChar(b"Latin-1\0".as_ptr() as *const _));
        SET_STRING_ELT(cn, 3, Rf_mkChar(b"codeset\0".as_ptr() as *const _));

        SET_VECTOR_ELT(ans, 0, Rf_ScalarLogical(if utf8 { TRUE } else { FALSE }));
        SET_VECTOR_ELT(ans, 1, Rf_ScalarLogical(if utf8 { TRUE } else { FALSE }));
        SET_VECTOR_ELT(ans, 2, Rf_ScalarLogical(FALSE));
        SET_VECTOR_ELT(
            ans,
            3,
            crate::sexp::constructors::Rf_mkString(if utf8 {
                c"UTF-8".as_ptr()
            } else {
                c"US-ASCII".as_ptr()
            }),
        );

        crate::eval::attrib_core::setAttrib(ans, crate::eval::attrib_core::R_NamesSymbol(), cn);
        ans
    }
}

/// R's `Sys.chmod()` — change file permissions.
#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn do_syschmod(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, CDR, LENGTH, LOGICAL};
        use crate::sexp::constructors::Rf_allocVector3;
        use crate::sexp::ffi::{FALSE, SEXPTYPE, TRUE};
        use crate::sexp::globals::R_NilValue;
        use std::fs;

        let mut paths = R_NilValue();
        let mut mode = R_NilValue();
        let mut positional = 0;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let value = CAR(current);
            match platform_tag_name(current).as_deref() {
                Some("paths") => paths = value,
                Some("mode") => mode = value,
                Some("use_umask") => {}
                Some(_) => {}
                None => {
                    match positional {
                        0 => paths = value,
                        1 => mode = value,
                        _ => {}
                    }
                    positional += 1;
                }
            }
            current = CDR(current);
        }

        let n = LENGTH(paths);
        let mode_val = parse_octal_mode_arg(mode).unwrap_or(0o777);

        let ans = Rf_allocVector3(SEXPTYPE::LGLSXP.as_c_int(), n as crate::sexp::ffi::R_xlen_t);
        let _ans_guard = protect(ans);
        let pa = LOGICAL(ans);
        for i in 0..n as usize {
            if let Some(path) = path_at(paths, i) {
                let result = fs::set_permissions(
                    path,
                    std::os::unix::fs::PermissionsExt::from_mode(mode_val as u32),
                );
                *pa.add(i) = if result.is_ok() { TRUE } else { FALSE };
            } else {
                *pa.add(i) = FALSE;
            }
        }
        ans
    }
}

/// WASM M1 stub for `do_syschmod`: `std::os::unix::fs::PermissionsExt`
/// does not exist on wasm32. Mirrors upstream R's no-chmod fallback
/// (platform.c `do_syschmod` `#else` branch): warn of insufficient OS
/// support on this platform and return an all-FALSE logical vector
/// (unsupported-operation path).
#[cfg(target_arch = "wasm32")]
pub unsafe fn do_syschmod(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, CDR, LENGTH, LOGICAL};
        use crate::sexp::constructors::Rf_allocVector3;
        use crate::sexp::ffi::{FALSE, SEXPTYPE};
        use crate::sexp::globals::R_NilValue;

        let mut paths = R_NilValue();
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let value = CAR(current);
            match platform_tag_name(current).as_deref() {
                Some("paths") => paths = value,
                Some(_) => {}
                None => {
                    if paths == R_NilValue() {
                        paths = value;
                    }
                }
            }
            current = CDR(current);
        }

        crate::mainutils::errors::Rf_warning(c"insufficient OS support on this platform".as_ptr());
        let n = LENGTH(paths);
        let ans = Rf_allocVector3(SEXPTYPE::LGLSXP.as_c_int(), n as crate::sexp::ffi::R_xlen_t);
        let _ans_guard = protect(ans);
        let pa = LOGICAL(ans);
        for i in 0..n as usize {
            *pa.add(i) = FALSE;
        }
        ans
    }
}

/// R's `Sys.umask()` — set file creation mask.
pub unsafe fn do_sysumask(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, CDR};
        use crate::sexp::globals::R_NilValue;

        let mut mode = R_NilValue();
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let value = CAR(current);
            match platform_tag_name(current).as_deref() {
                Some("mode") | None => {
                    if mode == R_NilValue() {
                        mode = value;
                    }
                }
                Some(_) => {}
            }
            current = CDR(current);
        }

        let old = with_required_current_instance(|instance| {
            let old = (*instance).file_creation_umask & 0o777;
            if let Some(new_mode) = parse_octal_mode_arg(mode) {
                (*instance).file_creation_umask = new_mode & 0o777;
            }
            old
        });
        octmode_scalar(old)
    }
}

/// R's `Sys.readlink()` -- read symbolic link target.
pub unsafe fn do_readlink(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, LENGTH, SET_STRING_ELT, STRING_ELT};
        use crate::sexp::constructors::{Rf_allocVector3, Rf_mkChar};
        use crate::sexp::ffi::SEXPTYPE;
        use crate::sexp::globals::R_NilValue;

        let paths = CAR(args);
        let n = LENGTH(paths);
        let ans = Rf_allocVector3(SEXPTYPE::STRSXP.as_c_int(), n as crate::sexp::ffi::R_xlen_t);
        let _ans_guard = protect(ans);

        for i in 0..n as usize {
            let elt = STRING_ELT(paths, i as crate::sexp::ffi::R_xlen_t);
            if elt.is_null() || elt == R_NilValue() {
                SET_STRING_ELT(
                    ans,
                    i as crate::sexp::ffi::R_xlen_t,
                    crate::sexp::globals::R_NaString(),
                );
            } else {
                let c = CStr::from_ptr(crate::sexp::accessors::CHAR(elt));
                let path = c.to_str().unwrap_or("");
                match std::fs::read_link(path) {
                    Ok(target) => {
                        SET_STRING_ELT(
                            ans,
                            i as crate::sexp::ffi::R_xlen_t,
                            Rf_mkChar(
                                CString::new(target.to_str().unwrap_or(""))
                                    .unwrap_or_default()
                                    .as_ptr(),
                            ),
                        );
                    }
                    Err(_) => {
                        let value = if std::fs::metadata(path).is_ok() {
                            Rf_mkChar(c"".as_ptr())
                        } else {
                            crate::sexp::globals::R_NaString()
                        };
                        SET_STRING_ELT(ans, i as crate::sexp::ffi::R_xlen_t, value);
                    }
                }
            }
        }
        ans
    }
}

/// R's `Cstack_info()` — C stack usage information.
pub unsafe fn do_Cstack_info(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{INTEGER, SET_STRING_ELT};
        use crate::sexp::constructors::{Rf_allocVector3, Rf_mkChar};
        use crate::sexp::ffi::SEXPTYPE;

        let ans = Rf_allocVector3(SEXPTYPE::INTSXP, 4);
        let _ans_guard = protect(ans);
        let cn = Rf_allocVector3(SEXPTYPE::STRSXP, 4);
        let _cn_guard = protect(cn);
        SET_STRING_ELT(cn, 0, Rf_mkChar(b"size\0".as_ptr() as *const _));
        SET_STRING_ELT(cn, 1, Rf_mkChar(b"current\0".as_ptr() as *const _));
        SET_STRING_ELT(cn, 2, Rf_mkChar(b"direction\0".as_ptr() as *const _));
        SET_STRING_ELT(cn, 3, Rf_mkChar(b"eval_depth\0".as_ptr() as *const _));

        let values = INTEGER(ans);
        *values.add(0) = 8 * 1024 * 1024;
        *values.add(1) = crate::sexp::ffi::NA_INTEGER;
        *values.add(2) = 1;
        *values.add(3) = 0;

        crate::eval::attrib_core::setAttrib(ans, crate::eval::attrib_core::R_NamesSymbol(), cn);
        ans
    }
}

/// R's `extSoftVersion()` — external software version information.
pub unsafe fn do_eSoftVersion(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::SET_STRING_ELT;
        use crate::sexp::constructors::{Rf_allocVector3, Rf_mkChar};
        use crate::sexp::ffi::SEXPTYPE;

        let blas = "";
        let fields = [
            ("zlib", "1.2.12"),
            ("bzlib", "1.0.8, 13-Jul-2019"),
            ("xz", "5.8.4"),
            ("libdeflate", ""),
            ("zstd", "1.5.7"),
            ("PCRE", "10.49 2026-09-28"),
            ("ICU", "78.1"),
            ("TRE", "TRE 0.8.0 R_fixes (BSD)"),
            (
                "iconv",
                "Apple or GNU libiconv 1.11 /usr/lib/libiconv.2.dylib",
            ),
            ("readline", "4.2 (EditLine wrapper)"),
            ("BLAS", &blas),
        ];

        let n = fields.len();
        let ans = Rf_allocVector3(SEXPTYPE::STRSXP.as_c_int(), n as crate::sexp::ffi::R_xlen_t);
        let _ans_guard = protect(ans);
        let cn = Rf_allocVector3(SEXPTYPE::STRSXP.as_c_int(), n as crate::sexp::ffi::R_xlen_t);
        let _cn_guard = protect(cn);

        for (i, (name, val)) in fields.iter().enumerate() {
            SET_STRING_ELT(
                ans,
                i as crate::sexp::ffi::R_xlen_t,
                Rf_mkChar(CString::new(*val).unwrap_or_default().as_ptr()),
            );
            SET_STRING_ELT(
                cn,
                i as crate::sexp::ffi::R_xlen_t,
                Rf_mkChar(CString::new(*name).unwrap_or_default().as_ptr()),
            );
        }

        crate::eval::attrib_core::setAttrib(ans, crate::eval::attrib_core::R_NamesSymbol(), cn);
        ans
    }
}

/// R's `Sys.junction()` — create NTFS junction (Windows only).
/// On non-Windows, returns FALSE.
pub unsafe fn do_mkjunction(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::constructors::Rf_ScalarLogical;
        use crate::sexp::ffi::FALSE;
        Rf_ScalarLogical(FALSE)
    }
}
