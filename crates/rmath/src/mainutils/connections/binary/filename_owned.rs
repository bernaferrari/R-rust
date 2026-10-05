//! Filename operations retain selected operands and their original authority.
#![forbid(unsafe_code)]

use crate::sexp::{
    SEXPTYPE,
    memory::TransientReservation,
    object::{Sexp, SexpError},
    owner::{RuntimeAccess, StoredOwner, with_runtime},
};
use std::{collections::HashSet, fs::File, io::Read};

struct Arguments(Vec<(Sexp<'static>, Sexp<'static>)>);

impl Arguments {
    fn capture(access: &RuntimeAccess, arguments: &Sexp<'static>) -> Result<Self, String> {
        let mut selected = Vec::new();
        let mut seen = HashSet::new();
        let mut cell = arguments.clone();
        while !cell.is_nil() {
            let identity = access.domain().link(&cell).map_err(|e| e.to_string())?;
            seen.try_reserve(1)
                .map_err(|_| "allocation failed while selecting binary arguments")?;
            if !seen.insert(identity) {
                return Err("cyclic binary argument list".into());
            }
            selected
                .try_reserve(1)
                .map_err(|_| "allocation failed while selecting binary arguments")?;
            selected.push((
                cell.try_tag()
                    .map_err(|e| e.to_string())?
                    .into_owned()
                    .map_err(|e| e.to_string())?,
                cell.try_car()
                    .map_err(|e| e.to_string())?
                    .into_owned()
                    .map_err(|e| e.to_string())?,
            ));
            cell = cell
                .try_cdr()
                .map_err(|e| e.to_string())?
                .into_owned()
                .map_err(|e| e.to_string())?;
        }
        Ok(Self(selected))
    }

    fn select(
        &self,
        position: usize,
        names: &[&str],
        default: &Sexp<'static>,
        access: &RuntimeAccess,
    ) -> Result<Sexp<'static>, String> {
        let domain = access.domain();
        let mut chosen = None;
        for (tag, value) in &self.0 {
            domain.link(tag).map_err(|e| e.to_string())?;
            domain.link(value).map_err(|e| e.to_string())?;
            if tag.is_nil() {
                continue;
            }
            let name = tag.try_printname().map_err(|e| e.to_string())?;
            for candidate in names {
                if name
                    .try_char_eq(candidate.as_bytes())
                    .map_err(|e| e.to_string())?
                {
                    chosen = Some(value);
                    break;
                }
            }
            if chosen.is_some() {
                break;
            }
        }
        let chosen = chosen.or_else(|| {
            self.0
                .iter()
                .filter(|(tag, _)| tag.is_nil())
                .nth(position)
                .map(|(_, value)| value)
        });
        Ok(match chosen {
            Some(value) if !value.is_nil() && *value != domain.missing() => value.clone(),
            _ => default.clone(),
        })
    }
}

fn execute<T>(
    anchor: &Sexp<'static>,
    operation: impl FnOnce(&RuntimeAccess) -> Result<T, String>,
) -> Result<T, String> {
    let authority = StoredOwner::from_value(anchor).map_err(|e| e.to_string())?;
    let owner = authority
        .managed()
        .ok_or_else(|| SexpError::RootUnavailable.to_string())?;
    let _pin = owner.pin().map_err(|e| e.to_string())?;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        with_runtime(&owner, operation).map_err(|e| e.to_string())?
    }));
    authority.require_active().map_err(|e| e.to_string())?;
    match result {
        Ok(result) => result,
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

fn run<T>(
    arguments: Sexp<'static>,
    operation: impl FnOnce(&RuntimeAccess, Arguments) -> Result<T, String>,
) -> Result<T, String> {
    execute(&arguments, |access| {
        operation(access, Arguments::capture(access, &arguments)?)
    })
}

fn filename(value: &Sexp<'static>, access: &RuntimeAccess) -> Result<String, String> {
    access.domain().link(value).map_err(|e| e.to_string())?;
    if value.typeof_() != SEXPTYPE::STRSXP || value.len() != 1 {
        return Err("invalid 'description' argument".into());
    }
    let name = value
        .try_string_value_elt(0)
        .map_err(|e| e.to_string())?
        .ok_or("invalid 'description' argument")?;
    access.require_active().map_err(|e| e.to_string())?;
    Ok(name)
}

fn integer(access: &RuntimeAccess, item: i32) -> Result<Sexp<'static>, String> {
    let domain = access.domain();
    let allocator = access.allocator(&domain).map_err(|e| e.to_string())?;
    let value = allocator
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 1)))
        .map_err(|e| e.to_string())?;
    let mut value = crate::sexp::SexpMut::try_from_checked(value).map_err(|e| e.to_string())?;
    value
        .try_set_integer_elt(0, item)
        .map_err(|e| e.to_string())?;
    Ok(value.freeze())
}

pub(super) fn reserve(
    access: &RuntimeAccess,
    bytes: usize,
) -> Result<TransientReservation, String> {
    access
        .with_arena(|arena| arena.try_reserve_transient(bytes))
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "binary file buffer exceeds session memory limit".into())
}

struct Buffer {
    bytes: Vec<u8>,
    _reservations: Vec<TransientReservation>,
}

enum Source {
    Browser(Buffer),
    Host(File),
}

impl Source {
    fn open(access: &RuntimeAccess, path: &str) -> Result<Self, String> {
        let (information, enabled) = access
            .with_native(|_| {
                Ok((
                    crate::mainutils::browser_files::info_current(path),
                    crate::mainutils::browser_files::enabled(),
                ))
            })
            .map_err(|e| e.to_string())?;
        if let Some(information) = information {
            let size = usize::try_from(information.size).map_err(|_| "binary file is too large")?;
            let reservation = reserve(access, size)?;
            let bytes = access
                .with_native(|_| Ok(crate::mainutils::browser_files::read_current(path)))
                .map_err(|e| e.to_string())?
                .ok_or("browser file not found")?;
            if bytes.len() != size {
                return Err("browser file changed while selecting binary bytes".into());
            }
            return Ok(Self::Browser(Buffer {
                bytes,
                _reservations: vec![reservation],
            }));
        }
        if enabled {
            return Err(format!("cannot open file '{path}': browser file not found"));
        }
        File::open(path)
            .map(Self::Host)
            .map_err(|error| format!("cannot open file '{path}': {error}"))
    }

    fn read(self, access: &RuntimeAccess, limit: Option<usize>) -> Result<Buffer, String> {
        match self {
            Self::Browser(mut buffer) => {
                if let Some(limit) = limit {
                    buffer.bytes.truncate(limit);
                }
                Ok(buffer)
            }
            Self::Host(mut file) => {
                let mut buffer = Buffer {
                    bytes: Vec::new(),
                    _reservations: Vec::new(),
                };
                if limit == Some(0) {
                    return Ok(buffer);
                }
                // Include the fixed stack workspace and the simultaneous old
                // and new byte buffers during try_reserve_exact reallocation.
                let stack = reserve(access, 8192)?;
                buffer
                    ._reservations
                    .try_reserve(1)
                    .map_err(|_| "allocation failed while admitting binary file bytes")?;
                buffer._reservations.push(stack);
                let mut admitted = 0usize;
                let mut chunk = [0u8; 8192];
                loop {
                    let remaining = limit.map_or(chunk.len(), |limit| {
                        limit.saturating_sub(buffer.bytes.len()).min(chunk.len())
                    });
                    if remaining == 0 {
                        break;
                    }
                    let count = match file.read(&mut chunk[..remaining]) {
                        Ok(0) => break,
                        Ok(count) => count,
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(error) => return Err(error.to_string()),
                    };
                    let peak = buffer
                        .bytes
                        .len()
                        .checked_add(count)
                        .and_then(|length| length.checked_mul(2))
                        .ok_or("binary file buffer is too large")?;
                    let reservation = reserve(access, peak.saturating_sub(admitted))?;
                    buffer
                        ._reservations
                        .try_reserve(1)
                        .map_err(|_| "allocation failed while admitting binary file bytes")?;
                    buffer
                        .bytes
                        .try_reserve_exact(count)
                        .map_err(|_| "allocation failed while reading binary file")?;
                    buffer._reservations.push(reservation);
                    buffer.bytes.extend_from_slice(&chunk[..count]);
                    admitted = peak;
                }
                Ok(buffer)
            }
        }
    }
}

pub(super) fn read(arguments: Sexp<'static>) -> Result<Sexp<'static>, String> {
    run(arguments, |access, arguments| {
        let domain = access.domain();
        let con = arguments.select(0, &["con"], &domain.nil(), access)?;
        let path = filename(&con, access)?;
        let source = Source::open(access, &path)?;
        let one = integer(access, 1)?;
        let na = integer(access, crate::sexp::ffi::NA_INTEGER)?;
        let what = arguments.select(1, &["what"], &domain.nil(), access)?;
        let count = arguments.select(2, &["n"], &one, access)?;
        let size = arguments.select(3, &["size"], &na, access)?;
        let signed = arguments.select(4, &["signed"], &domain.logical(true), access)?;
        let endian = arguments.select(5, &["endian", "swap"], &domain.missing(), access)?;
        let parameters =
            super::filename_read_parameters(access, [&what, &count, &size, &signed, &endian])
                .map_err(|e| e.to_string())?;
        let buffer = source.read(access, parameters.limit())?;
        let _workspace = reserve(access, parameters.workspace(buffer.bytes.len())?)?;
        super::filename_decode(access, &parameters, &buffer.bytes).map_err(|e| e.to_string())
    })
}

pub(super) fn write(arguments: Sexp<'static>) -> Result<Sexp<'static>, String> {
    run(arguments, |access, arguments| {
        let domain = access.domain();
        let object = arguments.select(0, &["object"], &domain.nil(), access)?;
        let con = arguments.select(1, &["con"], &domain.nil(), access)?;
        let path = filename(&con, access)?;
        let na = integer(access, crate::sexp::ffi::NA_INTEGER)?;
        let size = arguments.select(2, &["size"], &na, access)?;
        let endian = arguments.select(3, &["endian", "swap"], &domain.missing(), access)?;
        let use_bytes = arguments.select(4, &["useBytes"], &domain.logical(false), access)?;
        let (bytes, _reservations) =
            super::filename_encode(access, [&object, &size, &endian, &use_bytes])
                .map_err(|e| e.to_string())?;
        access
            .with_native(|_| {
                crate::mainutils::browser_files::write_text_or_host(&path, &bytes).map_err(
                    |error| SexpError::EvaluationFailed {
                        message: error.to_string(),
                    },
                )?;
                Ok(())
            })
            .map_err(|e| e.to_string())?;
        Ok(domain.nil())
    })
}

pub(super) fn source_bytes(con: Sexp<'static>, limit: Option<usize>) -> Result<Vec<u8>, String> {
    execute(&con, |access| {
        let path = filename(&con, access)?;
        Ok(Source::open(access, &path)?.read(access, limit)?.bytes)
    })
}

pub(super) fn sink_bytes(con: Sexp<'static>, bytes: &[u8]) -> Result<(), String> {
    execute(&con, |access| {
        let path = filename(&con, access)?;
        let _reservation = reserve(access, bytes.len())?;
        access
            .with_native(|_| {
                crate::mainutils::browser_files::write_text_or_host(&path, bytes).map_err(|error| {
                    SexpError::EvaluationFailed {
                        message: error.to_string(),
                    }
                })
            })
            .map_err(|e| e.to_string())
    })
}
