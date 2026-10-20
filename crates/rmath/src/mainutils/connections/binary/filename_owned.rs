//! Filename operations retain selected operands and their original authority.
#![forbid(unsafe_code)]

use crate::sexp::{
    SEXPTYPE,
    memory::TransientReservation,
    object::{Sexp, SexpError},
    owner::{RuntimeAccess, StoredOwner, with_runtime},
};
use std::{
    collections::HashSet,
    fs::File,
    io::{Read, Write},
};

type Formals = &'static [&'static [&'static str]];
const READ_FORMALS: Formals = &[
    &["con"],
    &["what"],
    &["n"],
    &["size"],
    &["signed"],
    &["endian", "swap"],
];
const WRITE_FORMALS: Formals = &[
    &["object"],
    &["con"],
    &["size"],
    &["endian", "swap"],
    &["useBytes"],
];

struct Arguments([Option<Sexp<'static>>; 6]);

impl Arguments {
    fn capture(
        access: &RuntimeAccess,
        arguments: &Sexp<'static>,
        formals: Formals,
    ) -> Result<Self, String> {
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
            let tag = cell
                .try_tag()
                .and_then(Sexp::into_owned)
                .map_err(|e| e.to_string())?;
            let value = cell
                .try_car()
                .and_then(Sexp::into_owned)
                .map_err(|e| e.to_string())?;
            // Admit every operand and tag before classification can invoke a
            // filename provider. A later select must not be the first check.
            access.domain().link(&tag).map_err(|e| e.to_string())?;
            access.domain().link(&value).map_err(|e| e.to_string())?;
            if !tag.is_nil() && tag.typeof_() != SEXPTYPE::SYMSXP {
                return Err("binary argument tag must be a symbol".into());
            }
            selected.push((tag, value));
            cell = cell
                .try_cdr()
                .map_err(|e| e.to_string())?
                .into_owned()
                .map_err(|e| e.to_string())?;
        }
        // Match named actuals first, then fill the remaining formal slots
        // with unnamed actuals in order. Named con does not consume what.
        let mut matched: [Option<Sexp<'static>>; 6] = std::array::from_fn(|_| None);
        for (tag, value) in selected.iter().filter(|(tag, _)| !tag.is_nil()) {
            let name = tag.try_printname().map_err(|e| e.to_string())?;
            let mut position = None;
            for (index, aliases) in formals.iter().enumerate() {
                for alias in *aliases {
                    if name
                        .try_char_eq(alias.as_bytes())
                        .map_err(|e| e.to_string())?
                    {
                        position = Some(index);
                    }
                }
            }
            let position = position.ok_or("unused binary argument")?;
            if matched[position].replace(value.clone()).is_some() {
                return Err(format!(
                    "formal argument '{}' matched by multiple actual arguments",
                    formals[position][0]
                ));
            }
        }
        for (tag, value) in selected {
            if tag.is_nil() {
                let position = matched[..formals.len()]
                    .iter()
                    .position(Option::is_none)
                    .ok_or("unused binary argument")?;
                matched[position] = Some(value);
            }
        }
        Ok(Self(matched))
    }

    fn select(
        &self,
        position: usize,
        default: &Sexp<'static>,
        access: &RuntimeAccess,
    ) -> Sexp<'static> {
        match &self.0[position] {
            Some(value) if !value.is_nil() && *value != access.domain().missing() => value.clone(),
            _ => default.clone(),
        }
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
    formals: Formals,
    operation: impl FnOnce(&RuntimeAccess, Arguments) -> Result<T, String>,
) -> Result<T, String> {
    execute(&arguments, |access| {
        operation(access, Arguments::capture(access, &arguments, formals)?)
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

/// Native source bytes and their original session's admission tokens.
/// Consumers retain this value through decoding and result publication.
pub struct Buffer {
    bytes: Vec<u8>,
    _reservations: Vec<TransientReservation>,
}

impl std::ops::Deref for Buffer {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.bytes
    }
}

impl Buffer {
    fn empty() -> Self {
        Self {
            bytes: Vec::new(),
            _reservations: Vec::new(),
        }
    }

    fn grow(&mut self, access: &RuntimeAccess, capacity: usize) -> Result<(), String> {
        if capacity <= self.bytes.capacity() {
            return Ok(());
        }
        // The old allocation and its admission remain live while the
        // replacement is allocated and copied. No arena loan spans growth.
        let reservation = reserve(access, capacity)?;
        let mut replacement = Vec::new();
        replacement
            .try_reserve_exact(capacity)
            .map_err(|_| "allocation failed while reading binary source")?;
        replacement.extend_from_slice(&self.bytes);
        self._reservations
            .try_reserve(1)
            .map_err(|_| "allocation failed while admitting binary source")?;
        self.bytes = replacement;
        self._reservations.push(reservation);
        Ok(())
    }
}

enum Source {
    Browser(Buffer),
    Host(File),
}

enum Sink {
    Raw,
    Connection(super::ConnectionToken),
    Browser(String),
    Host(File),
}

impl Sink {
    fn open(
        access: &RuntimeAccess,
        con: &Sexp<'static>,
        original: Option<super::ConnectionToken>,
    ) -> Result<Self, String> {
        access.domain().link(con).map_err(|e| e.to_string())?;
        if con.typeof_() == SEXPTYPE::RAWSXP {
            return Ok(Self::Raw);
        }
        if con.typeof_() != SEXPTYPE::STRSXP {
            let selected =
                super::binary_connection_index(access, con, true).map_err(|e| e.to_string())?;
            if let Some(original) = original {
                if !original.same_connection(&selected) {
                    return Err("original connection is no longer available".into());
                }
                return Ok(Self::Connection(original));
            }
            return Ok(Self::Connection(selected));
        }
        let path = filename(con, access)?;
        let browser = access
            .with_native(|_| {
                Ok(crate::mainutils::browser_files::enabled()
                    || crate::mainutils::browser_files::contains_current(&path))
            })
            .map_err(|e| e.to_string())?;
        if browser {
            access
                .with_native(|_| {
                    crate::mainutils::browser_files::write_current(&path, &[])
                        .map_err(|message| SexpError::EvaluationFailed { message })
                })
                .map_err(|e| e.to_string())?;
            Ok(Self::Browser(path))
        } else {
            File::create(path)
                .map(Self::Host)
                .map_err(|e| e.to_string())
        }
    }

    fn write(mut self, access: &RuntimeAccess, bytes: &[u8]) -> Result<Sexp<'static>, String> {
        match &mut self {
            Self::Raw => return super::raw_sink_result(access, bytes).map_err(|e| e.to_string()),
            Self::Connection(token) => access
                .with_native(|_| {
                    let index = token
                        .require_current()
                        .map_err(|message| SexpError::EvaluationFailed { message })?;
                    super::connection_write_bytes(index, bytes);
                    Ok(())
                })
                .map_err(|e| e.to_string()),
            Self::Browser(path) => access
                .with_native(|_| {
                    crate::mainutils::browser_files::write_current(path, bytes)
                        .map_err(|message| SexpError::EvaluationFailed { message })
                })
                .map_err(|e| e.to_string()),
            Self::Host(file) => file.write_all(bytes).map_err(|e| e.to_string()),
        }?;
        Ok(access.domain().nil())
    }
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

    fn read(
        self,
        access: &RuntimeAccess,
        limit: Option<usize>,
        strings: Option<usize>,
    ) -> Result<Buffer, String> {
        match self {
            Self::Browser(mut buffer) => {
                if let Some(limit) = limit {
                    buffer.bytes.truncate(limit);
                }
                if let Some(mut strings) = strings {
                    let count = if strings == 0 {
                        0
                    } else {
                        buffer
                            .bytes
                            .iter()
                            .position(|byte| {
                                if *byte == 0 {
                                    strings -= 1;
                                }
                                strings == 0
                            })
                            .map_or(buffer.bytes.len(), |index| index + 1)
                    };
                    buffer.bytes.truncate(count);
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
                let mut remaining_strings = strings;
                let mut chunk = [0u8; 8192];
                loop {
                    let remaining = limit.map_or(chunk.len(), |limit| {
                        limit.saturating_sub(buffer.bytes.len()).min(chunk.len())
                    });
                    if remaining == 0 {
                        break;
                    }
                    let mut count = match file.read(&mut chunk[..remaining]) {
                        Ok(0) => break,
                        Ok(count) => count,
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(error) => return Err(error.to_string()),
                    };
                    if let Some(remaining) = &mut remaining_strings {
                        if *remaining == 0 {
                            break;
                        }
                        if let Some(index) = chunk[..count].iter().position(|byte| {
                            if *byte == 0 {
                                *remaining -= 1;
                            }
                            *remaining == 0
                        }) {
                            count = index + 1;
                        }
                    }
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
                    if remaining_strings == Some(0) {
                        break;
                    }
                }
                Ok(buffer)
            }
        }
    }
}

pub(super) fn read(arguments: Sexp<'static>) -> Result<Sexp<'static>, String> {
    run(arguments, READ_FORMALS, |access, arguments| {
        let domain = access.domain();
        let con = arguments.select(0, &domain.nil(), access);
        let mut connection = None;
        let source = if con.typeof_() == SEXPTYPE::STRSXP {
            let path = filename(&con, access)?;
            Some(Source::open(access, &path)?)
        } else {
            if con.typeof_() != SEXPTYPE::RAWSXP {
                connection = Some(
                    super::binary_connection_index(access, &con, false)
                        .map_err(|e| e.to_string())?,
                );
            }
            None
        };
        let one = integer(access, 1)?;
        let na = integer(access, crate::sexp::ffi::NA_INTEGER)?;
        let what = arguments.select(1, &domain.nil(), access);
        let count = arguments.select(2, &one, access);
        let size = arguments.select(3, &na, access);
        let signed = arguments.select(4, &domain.logical(true), access);
        let endian = arguments.select(5, &domain.missing(), access);
        let parameters =
            super::filename_read_parameters(access, [&what, &count, &size, &signed, &endian])
                .map_err(|e| e.to_string())?;
        let buffer = match source {
            Some(source) => source.read(access, parameters.limit(), parameters.strings())?,
            None => {
                source_bytes_bounded(con, parameters.limit(), parameters.strings(), connection)?
            }
        };
        let _workspace = reserve(access, parameters.workspace(buffer.bytes.len())?)?;
        super::filename_decode(access, &parameters, &buffer.bytes).map_err(|e| e.to_string())
    })
}

pub(super) fn write(arguments: Sexp<'static>) -> Result<Sexp<'static>, String> {
    run(arguments, WRITE_FORMALS, |access, arguments| {
        let domain = access.domain();
        let object = arguments.select(0, &domain.nil(), access);
        let con = arguments.select(1, &domain.nil(), access);
        let na = integer(access, crate::sexp::ffi::NA_INTEGER)?;
        let size = arguments.select(2, &na, access);
        let endian = arguments.select(3, &domain.missing(), access);
        let use_bytes = arguments.select(4, &domain.logical(false), access);
        let mut sink = None;
        let original = std::cell::RefCell::new(None);
        let (bytes, _reservations) = super::filename_encode_staged(
            access,
            [&object, &size, &endian, &use_bytes],
            || {
                if !matches!(con.typeof_(), SEXPTYPE::RAWSXP | SEXPTYPE::STRSXP) {
                    let token = super::binary_connection_index(access, &con, true)
                        .map_err(|e| e.to_string())?;
                    *original.borrow_mut() = Some(token);
                }
                Ok(())
            },
            || {
                let original = original.borrow().clone();
                sink = Some(Sink::open(access, &con, original)?);
                Ok(())
            },
        )
        .map_err(|e| e.to_string())?;
        sink.ok_or("binary sink was not opened")?
            .write(access, &bytes)
    })
}

pub(super) fn source_bytes(con: Sexp<'static>, limit: Option<usize>) -> Result<Buffer, String> {
    source_bytes_bounded(con, limit, None, None)
}

fn source_bytes_bounded(
    con: Sexp<'static>,
    limit: Option<usize>,
    strings: Option<usize>,
    original: Option<super::ConnectionToken>,
) -> Result<Buffer, String> {
    execute(&con, |access| {
        access.domain().link(&con).map_err(|e| e.to_string())?;
        if con.typeof_() == SEXPTYPE::STRSXP {
            let path = filename(&con, access)?;
            return Source::open(access, &path)?.read(access, limit, strings);
        }
        let mut buffer = Buffer::empty();
        if con.typeof_() == SEXPTYPE::RAWSXP {
            let count = usize::try_from(con.len()).map_err(|_| "binary source is too large")?;
            let count = limit.map_or(count, |limit| limit.min(count));
            if strings.is_none() {
                buffer.grow(access, count)?;
            }
            let mut remaining_strings = strings;
            for index in 0..count {
                if remaining_strings == Some(0) {
                    break;
                }
                if strings.is_some() {
                    let next = index.checked_add(1).ok_or("binary source is too large")?;
                    let capacity = next.checked_next_power_of_two().unwrap_or(next).min(count);
                    buffer.grow(access, capacity)?;
                }
                buffer.bytes.push(
                    con.try_raw_elt(index as crate::sexp::ffi::R_xlen_t)
                        .map_err(|e| e.to_string())?,
                );
                if buffer.bytes.last() == Some(&0)
                    && let Some(remaining) = &mut remaining_strings
                {
                    *remaining -= 1;
                }
                access.require_active().map_err(|e| e.to_string())?;
            }
            return Ok(buffer);
        }
        let selected = super::source_connection_index(access, &con).map_err(|e| e.to_string())?;
        let token = if let Some(original) = original {
            if !original.same_connection(&selected) {
                return Err("original connection is no longer available".into());
            }
            original
        } else {
            selected
        };
        let mut remaining_strings = strings;
        while limit.is_none_or(|limit| buffer.bytes.len() < limit) && remaining_strings != Some(0) {
            let next = buffer
                .bytes
                .len()
                .checked_add(1)
                .ok_or("binary source is too large")?;
            let capacity = next.checked_next_power_of_two().unwrap_or(next);
            buffer.grow(access, limit.map_or(capacity, |limit| capacity.min(limit)))?;
            let byte = access
                .with_native(|_| {
                    let index = token
                        .require_current()
                        .map_err(|message| SexpError::EvaluationFailed { message })?;
                    Ok(super::connection_fgetc(index))
                })
                .map_err(|e| e.to_string())?;
            if byte < 0 {
                break;
            }
            buffer.bytes.push(byte as u8);
            if byte == 0
                && let Some(remaining) = &mut remaining_strings
            {
                *remaining -= 1;
            }
        }
        Ok(buffer)
    })
}

pub(super) fn decode_source(
    con: Sexp<'static>,
    parameters: &super::FilenameReadParameters,
    buffer: &Buffer,
) -> Result<Sexp<'static>, String> {
    execute(&con, |access| {
        let _workspace = reserve(access, parameters.workspace(buffer.bytes.len())?)?;
        super::filename_decode(access, parameters, &buffer.bytes).map_err(|e| e.to_string())
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
