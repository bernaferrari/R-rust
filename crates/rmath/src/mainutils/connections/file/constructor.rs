//! File arguments are retained before any element provider or R allocation.
#![forbid(unsafe_code)]

use crate::sexp::{
    SEXPTYPE,
    ffi::NA_INTEGER,
    object::{Sexp, SexpError, SexpResult},
    owner::RuntimeAccess,
};
use std::collections::HashSet;

pub(super) struct FileOptions {
    pub description: String,
    pub open: String,
    pub raw: bool,
}

pub(super) fn prepare(access: &RuntimeAccess, arguments: Sexp<'static>) -> SexpResult<FileOptions> {
    let domain = access.domain();
    let mut values = Vec::new();
    let mut seen = HashSet::new();
    let mut cell = arguments;
    while !cell.is_nil() {
        let identity = domain.link(&cell)?;
        if !seen.insert(identity) {
            return Err(invalid("cyclic file argument list"));
        }
        values.push((cell.try_tag()?.into_owned()?, cell.try_car()?.into_owned()?));
        cell = cell.try_cdr()?.into_owned()?;
    }
    // Resolve every operand before invoking even the first element provider.
    // The snapshots, rather than the mutable argument chain, retain all values.
    let names = [
        "description",
        "open",
        "blocking",
        "encoding",
        "method",
        "raw",
    ];
    let mut selected: [Option<Sexp<'static>>; 6] = std::array::from_fn(|_| None);
    for (tag, value) in &values {
        if !tag.is_nil() {
            let name = tag.try_printname()?;
            for (index, formal) in names.iter().enumerate() {
                if name.try_char_eq(formal.as_bytes())? {
                    selected[index] = Some(value.clone());
                    break;
                }
            }
        }
    }
    let mut position = 0;
    for (tag, value) in &values {
        if !tag.is_nil() {
            continue;
        }
        while position < selected.len() && selected[position].is_some() {
            position += 1;
        }
        if position == selected.len() {
            break;
        }
        selected[position] = Some(value.clone());
        position += 1;
    }
    let [description, open, blocking, encoding, _method, raw] =
        selected.map(|value| value.filter(|value| !value.is_nil() && *value != domain.missing()));

    let description = string(access, description.as_ref(), "description", "")?;
    let open = string(access, open.as_ref(), "open", "")?;
    // GNU admits blocking before encoding and raw; keep provider order too.
    logical(access, blocking.as_ref(), "blocking", true)?;
    let encoding = match encoding.as_ref() {
        Some(value) if value.typeof_() == SEXPTYPE::STRSXP && value.is_empty() => {
            "native.enc".to_string()
        }
        value => string(access, value, "encoding", "native.enc")?,
    };
    if encoding == "unknown" {
        return Err(invalid("invalid connection"));
    }
    let raw = logical(access, raw.as_ref(), "raw", false)?;
    access.require_active()?;
    Ok(FileOptions {
        description,
        open,
        raw,
    })
}

fn invalid(message: &str) -> SexpError {
    SexpError::EvaluationFailed {
        message: message.into(),
    }
}

fn string(
    access: &RuntimeAccess,
    value: Option<&Sexp<'static>>,
    name: &str,
    default: &str,
) -> SexpResult<String> {
    let Some(value) = value else {
        return Ok(default.into());
    };
    if value.typeof_() != SEXPTYPE::STRSXP || value.len() != 1 {
        return Err(invalid(&format!("invalid '{name}' argument")));
    }
    let selected = value.try_string_value_elt(0)?;
    access.require_active()?;
    selected.ok_or_else(|| invalid(&format!("invalid '{name}' argument")))
}

fn logical(
    access: &RuntimeAccess,
    value: Option<&Sexp<'static>>,
    name: &str,
    default: bool,
) -> SexpResult<bool> {
    let Some(value) = value else {
        return Ok(default);
    };
    let logical = super::file_logical(access, value, name == "raw")?;
    if logical == NA_INTEGER {
        return Err(invalid(&if name == "raw" {
            "NA in coercion to boolean".into()
        } else {
            format!("invalid '{name}' argument")
        }));
    }
    Ok(logical != 0)
}
