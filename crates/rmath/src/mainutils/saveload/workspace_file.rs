//! Complete workspace snapshots and file publication.

use super::*;
use crate::mainutils::{browser_files, serialize};
use crate::sexp::{object::PairlistBuilder, owner::OwnerToken};

pub(super) unsafe fn save(
    owner: OwnerToken<'_>,
    names: &[String],
    file: SEXP,
    envir: SEXP,
    options: SaveOptions,
) {
    let path = unsafe { workspace_path(file, "'file' must be a non-empty character string") };
    if unsafe { TYPEOF(envir) } != SEXPTYPE::ENVSXP {
        crate::sexp::context::r_error("invalid 'envir' argument");
    }
    let envir = owner.node_factory().wrap(envir).unwrap_or_else(fail);
    let mut bindings = PairlistBuilder::new_in(owner);
    for name in names {
        let native = std::ffi::CString::new(name.as_str()).unwrap_or_else(fail);
        if name.is_empty() {
            crate::sexp::context::r_error("invalid object name");
        }
        let symbol = unsafe { Rf_install(native.as_ptr()) };
        let tag = bindings.wrap(symbol).unwrap_or_else(fail);
        let mut value = unsafe { R_findVar(symbol, envir.as_raw()) };
        if value == R_UnboundValue() {
            crate::sexp::context::r_error(format!("object ‘{name}’ not found"));
        }
        if options.eval_promises && unsafe { TYPEOF(value) } == SEXPTYPE::PROMSXP {
            let promise = bindings.wrap(value).unwrap_or_else(fail);
            value = unsafe { crate::eval::eval::Rf_eval(promise.as_raw(), envir.as_raw()) };
            owner.require_active().unwrap_or_else(fail);
        }
        let value = bindings.wrap(value).unwrap_or_else(fail);
        bindings.push(value, Some(tag)).unwrap_or_else(fail);
    }
    let bindings = bindings.finish().unwrap_or_else(fail);
    let plain = unsafe {
        serialize::serialize_workspace_snapshot(
            &owner,
            bindings.as_raw(),
            options.version,
            options.ascii,
        )
    };
    let compressed = options.codec.map(|codec| {
        serialize::compress_container(&owner, &plain, codec)
            .unwrap_or_else(|error| fail(format!("cannot compress workspace: {error}")))
    });
    let payload = compressed.as_deref().unwrap_or(&plain);
    owner.require_active().unwrap_or_else(fail);
    let result = if browser_files::enabled() || browser_files::contains_current(&path) {
        browser_files::write_current(&path, payload).map_err(io::Error::other)
    } else {
        crate::mainutils::platform::publish_host_file(&path, |file| file.write_all(payload))
    };
    result.unwrap_or_else(|error| fail(format!("cannot save workspace to '{path}': {error}")));
}

pub(super) struct SaveOptions {
    pub version: c_int,
    pub ascii: bool,
    pub codec: Option<&'static str>,
    pub eval_promises: bool,
}

pub(super) unsafe fn load(owner: OwnerToken<'_>, file: SEXP, envir: SEXP) -> SEXP {
    let path = unsafe { workspace_path(file, "bad 'file' argument") };
    if envir == R_NilValue() {
        crate::sexp::context::r_error("use of NULL environment is defunct");
    }
    let target = owner.node_factory().wrap(envir).unwrap_or_else(fail);
    if !target.is_environment() {
        crate::sexp::context::r_error("invalid 'envir' argument");
    }
    let open_error = |error: &dyn std::fmt::Display| -> ! {
        crate::sexp::context::r_error(format!("cannot open workspace '{path}': {error}"))
    };
    let browser;
    let host;
    let input: &[u8] = if browser_files::enabled() || browser_files::contains_current(&path) {
        browser = browser_files::open_snapshot_current(&path, browser_files::BrowserOpenMode::Read)
            .unwrap_or_else(|error| open_error(&error));
        &browser
    } else {
        host = browser_files::read_admitted_bytes_or_host(&path)
            .unwrap_or_else(|error| open_error(&error));
        &host
    };
    let expanded = serialize::expand_workspace_snapshot(&owner, input)
        .unwrap_or_else(|error| fail(format!("cannot read workspace '{path}': {error}")));
    let bytes = expanded.as_deref().unwrap_or(input);
    if bytes.is_empty() {
        crate::sexp::context::r_error("empty (zero-byte) input file");
    }
    let body = bytes.get(5..).unwrap_or_default();
    let bindings = match bytes.get(..5) {
        Some(b"RDX2\n" | b"RDX3\n") if body.starts_with(b"X\n") => {
            serialize::decode_workspace_snapshot(&owner, body, false).unwrap_or_else(fail)
        }
        Some(b"RDB2\n" | b"RDB3\n") if body.starts_with(b"B\n") => {
            serialize::decode_workspace_snapshot(&owner, body, false).unwrap_or_else(fail)
        }
        Some(b"RDA2\n" | b"RDA3\n") if body.starts_with(b"A\n") => {
            serialize::decode_workspace_snapshot(&owner, body, true).unwrap_or_else(fail)
        }
        Some(b"RDA1\n" | b"RDA2\n" | b"RDA3\n") if body.first().is_some_and(u8::is_ascii_digit) => {
            read_legacy_workspace(owner, body)
                .unwrap_or_else(fail)
                .into_owned()
                .unwrap_or_else(fail)
        }
        _ => crate::sexp::context::r_error(
            "bad restore file magic number (file may be corrupted) -- no data loaded",
        ),
    };

    let mut len: R_xlen_t = 0;
    let mut cell = bindings.clone();
    // GNU serializes the top-level workspace as an unshared pairlist. Bound
    // its length independently and check every tag before preparing updates.
    while !cell.is_nil() {
        if cell.typeof_() != SEXPTYPE::LISTSXP
            || !cell.try_tag().unwrap_or_else(fail).is_symbol()
            || len as usize >= body.len()
        {
            crate::sexp::context::r_error("invalid workspace binding list");
        }
        len += 1;
        cell = cell.try_cdr().unwrap_or_else(fail);
    }
    let names = unsafe { Rf_allocVector3(SEXPTYPE::STRSXP, len) };
    let _names = unsafe { protect(names) };
    let mut cell = bindings.clone();
    for index in 0..len {
        let tag = cell.try_tag().unwrap_or_else(fail);
        let name = unsafe { PRINTNAME(tag.as_raw()) };
        if name.is_null()
            || name == R_NaString()
            || unsafe { TYPEOF(name) } != SEXPTYPE::CHARSXP
            || unsafe { *CHAR(name) } == 0
        {
            crate::sexp::context::r_error("invalid workspace binding name");
        }
        unsafe { SET_STRING_ELT(names, index, name) };
        cell = cell.try_cdr().unwrap_or_else(fail);
    }
    if len != 0 {
        crate::sexp::envir::publish_workspace_bindings(owner, &target, &bindings, len as usize)
            .unwrap_or_else(fail);
    }
    names
}

fn fail<T>(error: impl std::fmt::Display) -> T {
    crate::sexp::context::r_error(error.to_string())
}

unsafe fn workspace_path(file: SEXP, diagnostic: &str) -> String {
    unsafe {
        if file.is_null() || TYPEOF(file) != SEXPTYPE::STRSXP || XLENGTH(file) != 1 {
            crate::sexp::context::r_error(diagnostic);
        }
        let element = STRING_ELT(file, 0);
        if element.is_null() || element == R_NaString() {
            crate::sexp::context::r_error(diagnostic);
        }
        let path = std::ffi::CStr::from_ptr(CHAR(element))
            .to_str()
            .unwrap_or_else(fail);
        if path.is_empty() {
            crate::sexp::context::r_error(diagnostic);
        }
        path.to_owned()
    }
}
