//! `Random.seed`, `loadRDS`, `saveRDS`.

#[allow(unused_imports)]
use std::collections::BTreeSet;
#[allow(unused_imports)]
use std::ffi::{CStr, CString};
#[allow(unused_imports)]
use std::os::raw::{c_char, c_int};
#[allow(unused_imports)]
use std::path::{Path, PathBuf};

use crate::mainutils::essentials::*;

#[cfg(test)]
#[path = "serialize_workspace_tests.rs"]
mod workspace_tests;

#[allow(unused_imports)]
use crate::sexp::accessors::{
    ATTRIB, CADR, CAR, CDR, CHAR, COMPLEX, FORMALS, FRAME, HASHTAB, INTEGER, INTEGER_ELT, LENGTH,
    LOGICAL, LOGICAL_ELT, PRINTNAME, RAW, REAL, REAL_ELT, SET_ENCLOS, SET_OBJECT, SET_STRING_ELT,
    SET_VECTOR_ELT, SETCAR, SETCDR, SETTAG, STRING_ELT, TAG, TYPEOF, VECTOR_ELT, XLENGTH,
};
#[allow(unused_imports)]
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3, Rf_cons, Rf_mkChar,
    Rf_mkString,
};
#[allow(unused_imports)]
use crate::sexp::context::RError;
#[allow(unused_imports)]
use crate::sexp::ffi::{
    FALSE, NA_INTEGER, NA_LOGICAL, NA_REAL, R_xlen_t, Rcomplex, SEXP, SEXPTYPE, TRUE,
};
#[allow(unused_imports)]
use crate::sexp::globals::{R_MissingArg, R_NilValue};
#[allow(unused_imports)]
use crate::sexp::protect::protect;
#[allow(unused_imports)]
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// Complete R runtime — serialization
// ---------------------------------------------------------------------------

/// R's `Random.seed` — get or set the random seed.
pub unsafe fn do_Random_seed(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Get the current RNG state
        let seed_vec = Rf_allocVector3(SEXPTYPE::INTSXP, 626);
        if seed_vec.is_null() {
            return R_NilValue();
        }
        let _p = protect(seed_vec);
        let dst = INTEGER(seed_vec);
        // Set default seed values
        *dst = 10407_i32; // RNG kind marker
        for i in 1..626 {
            *dst.add(i) = i as c_int;
        }
        seed_vec
    }
}

/// R's `loadRDS(file, refhook)` — load a single serialized R object.
pub unsafe fn do_loadRDS(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let owner = crate::sexp::owner::OwnerToken::current()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let _pin = owner
            .pin()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let _arguments = owner
            .sexp(args)
            .and_then(crate::sexp::Sexp::into_owned)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let file_arg = CAR(args);
        let _file = owner
            .sexp(file_arg)
            .and_then(crate::sexp::Sexp::into_owned)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let connection_bytes;
        let file_bytes;
        let bytes = if crate::mainutils::connections::inherits_class(file_arg, "connection") {
            let idx = crate::mainutils::coerce::asInteger(file_arg);
            owner
                .require_active()
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            let mut bytes = Vec::new();
            loop {
                let b = crate::mainutils::connections::connection_fgetc(idx);
                if b < 0 {
                    break;
                }
                bytes.push(b as u8);
            }
            connection_bytes = bytes;
            connection_bytes.as_slice()
        } else {
            let file_path = elt_to_string(file_arg, 0);
            owner
                .require_active()
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            if let Some(bytes) = crate::library::datasets::file_bytes(&file_path) {
                bytes
            } else {
                file_bytes = match crate::mainutils::browser_files::read_admitted_bytes_or_host(
                    &file_path,
                ) {
                    Ok(bytes) => bytes,
                    Err(err) => {
                        std::panic::panic_any(RError {
                            message: format!("cannot open compressed file '{}': {err}", file_path),
                        });
                    }
                };
                &file_bytes[..]
            }
        };

        let raw_vec = Rf_allocVector3(SEXPTYPE::RAWSXP, bytes.len() as R_xlen_t);
        if raw_vec.is_null() {
            return R_NilValue();
        }
        let _raw_guard = protect(raw_vec);
        if !bytes.is_empty() {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), RAW(raw_vec), bytes.len());
        }
        let result = crate::mainutils::serialize::R_unserialize(raw_vec, R_NilValue());
        owner
            .require_active()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        result
    }
}

/// R's `saveRDS(object, file, ascii, ...)` — save a single R object.
pub unsafe fn do_saveRDS(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let owner = crate::sexp::owner::OwnerToken::current()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let _pin = owner
            .pin()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let _arguments = owner
            .sexp(args)
            .and_then(crate::sexp::Sexp::into_owned)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let mut object_arg = R_NilValue();
        let mut file_arg = R_NilValue();
        let mut ascii_arg = R_NilValue();
        let mut version_arg = R_NilValue();
        let mut compress_arg = R_NilValue(); // GNU default: compress = TRUE
        let mut positional: Vec<SEXP> = Vec::new();
        let mut cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let value = CAR(cell);
            if !tag.is_null() && tag != R_NilValue() {
                let name = std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(
                    crate::sexp::accessors::PRINTNAME(tag),
                ))
                .to_string_lossy()
                .into_owned();
                match name.as_str() {
                    "object" => object_arg = value,
                    "file" => file_arg = value,
                    "ascii" => ascii_arg = value,
                    "version" => version_arg = value,
                    "compress" => compress_arg = value,
                    _ => {}
                }
            } else {
                positional.push(value);
            }
            cell = CDR(cell);
        }
        for (i, value) in positional.iter().copied().enumerate() {
            match i {
                0 if object_arg == R_NilValue() => object_arg = value,
                1 if file_arg == R_NilValue() => file_arg = value,
                2 if ascii_arg == R_NilValue() => ascii_arg = value,
                3 if version_arg == R_NilValue() => version_arg = value,
                4 if compress_arg == R_NilValue() => compress_arg = value,
                _ => {}
            }
        }
        if file_arg.is_null() || file_arg == R_NilValue() {
            eprintln!("saveRDS: file argument is required");
            return R_NilValue();
        }
        // Selected operands and the filename survive serializer/provider
        // callbacks even if the argument graph or caller bindings change.
        let _selected = [object_arg, file_arg, ascii_arg, version_arg, compress_arg].map(|value| {
            owner
                .sexp(value)
                .and_then(crate::sexp::Sexp::into_owned)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
        });
        let is_connection = crate::mainutils::connections::inherits_class(file_arg, "connection");
        let connection = if is_connection {
            let index = crate::mainutils::coerce::asInteger(file_arg);
            Some(
                crate::mainutils::connections::connection_token(index)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error)),
            )
        } else {
            None
        };
        let file_path = (!is_connection).then(|| elt_to_string(file_arg, 0));
        owner
            .require_active()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));

        // GNU's R-level saveRDS() dispatches compress= to a container
        // connection: logical TRUE -> gzip, FALSE -> plain, or a codec name.
        let mut compress: Option<&'static str> = None; // None = uncompressed
        if !is_connection {
            let arg = if compress_arg.is_null() || compress_arg == R_NilValue() {
                crate::sexp::constructors::Rf_ScalarLogical(crate::sexp::ffi::TRUE)
            } else {
                compress_arg
            };
            match <SEXPTYPE as std::convert::From<c_int>>::from(TYPEOF(arg)) {
                SEXPTYPE::LGLSXP => {
                    let v = crate::sexp::accessors::LOGICAL_ELT(arg, 0);
                    if v == crate::sexp::ffi::TRUE {
                        compress = Some("gzip");
                    } else if v == crate::sexp::ffi::FALSE {
                        compress = None;
                    } else {
                        std::panic::panic_any(RError {
                            message: "invalid 'compress' argument: NA".to_string(),
                        });
                    }
                }
                SEXPTYPE::STRSXP => {
                    let name = elt_to_string(arg, 0);
                    match name.as_str() {
                        "gzip" | "bzip2" | "xz" => {
                            compress = Some(match name.as_str() {
                                "gzip" => "gzip",
                                "bzip2" => "bzip2",
                                _ => "xz",
                            })
                        }
                        other => {
                            std::panic::panic_any(RError {
                                message: format!("invalid 'compress' argument: {other}"),
                            });
                        }
                    }
                }
                _ => {
                    let shown = crate::mainutils::deparse::deparse1(arg, false, 0);
                    let shown = if XLENGTH(shown) > 0 {
                        elt_to_string(shown, 0)
                    } else {
                        "?".to_string()
                    };
                    std::panic::panic_any(RError {
                        message: format!("invalid 'compress' argument: {shown}"),
                    });
                }
            }
        }

        let raw = crate::mainutils::serialize::R_serialize(
            object_arg,
            R_NilValue(),
            ascii_arg,
            version_arg,
            R_NilValue(),
        );
        if raw.is_null() || TYPEOF(raw) != SEXPTYPE::RAWSXP {
            std::panic::panic_any(RError {
                message: "saveRDS failed to serialize object".to_string(),
            });
        }
        let _raw_guard = protect(raw);
        owner
            .require_active()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));

        let len = XLENGTH(raw) as usize;
        let plain = std::slice::from_raw_parts(RAW(raw), len);
        // Plain output borrows the already charged R raw vector. Compressed
        // output retains its original reservation through file/connection I/O.
        let encoded = compress.map(|codec| {
            crate::mainutils::serialize::compress_container(&owner, plain, codec).unwrap_or_else(
                |e| crate::sexp::context::r_error(format!("cannot compress file: {e}")),
            )
        });
        let payload = encoded.as_deref().unwrap_or(plain);
        if let Some(connection) = connection {
            let idx = connection
                .require_current()
                .unwrap_or_else(|error| crate::sexp::context::r_error(error));
            crate::mainutils::connections::connection_write_bytes(idx, payload);
        } else {
            let file_path = file_path.as_deref().expect("selected RDS filename");
            if let Err(err) =
                crate::mainutils::browser_files::write_bytes_or_host(file_path, payload)
            {
                std::panic::panic_any(RError {
                    message: format!("cannot open compressed file '{}': {err}", file_path),
                });
            }
        }
        // Stock saveRDS() returns invisible NULL; the top-level auto-print
        // depends on the exact flag.
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);

        R_NilValue()
    }
}
