//! Bytes and complete binding graphs remain owned through all callback stages.
#![forbid(unsafe_code)]
use crate::sexp::{
    SEXPTYPE,
    memory::TransientReservation,
    object::{Sexp, SexpError},
    owner::{RuntimeAccess, StoredOwner, with_runtime},
};
use std::{io::Read, path::Path};

fn allocation_error() -> String {
    "allocation failed while reading serialized package data".into()
}

struct Bytes {
    data: Vec<u8>,
    _reservations: Vec<TransientReservation>,
}

fn read(access: &RuntimeAccess, mut reader: impl Read) -> Result<Bytes, String> {
    let mut output = Bytes {
        data: Vec::new(),
        _reservations: Vec::new(),
    };
    let mut chunk = [0; 8192];
    loop {
        let count = reader
            .read(&mut chunk)
            .map_err(|e| format!("could not read serialized package data: {e}"))?;
        if count == 0 {
            break;
        }
        let reservation = access
            .with_arena(|arena| arena.try_reserve_transient(count))
            .map_err(|e| e.to_string())?
            .ok_or_else(allocation_error)?;
        output
            .data
            .try_reserve_exact(count)
            .map_err(|_| allocation_error())?;
        output
            ._reservations
            .try_reserve(1)
            .map_err(|_| allocation_error())?;
        output.data.extend_from_slice(&chunk[..count]);
        output._reservations.push(reservation);
    }
    access.require_active().map_err(|e| e.to_string())?;
    Ok(output)
}

fn payload(bytes: &[u8]) -> Result<&[u8], String> {
    let header = bytes
        .get(..5)
        .ok_or("bad restore file magic number (file may be corrupted) -- no data loaded")?;
    if !matches!(
        header,
        b"RDX2\n" | b"RDX3\n" | b"RDA2\n" | b"RDA3\n" | b"RDB2\n" | b"RDB3\n"
    ) {
        return Err(
            "bad restore file magic number (file may be corrupted) -- no data loaded".into(),
        );
    }
    let payload = &bytes[5..];
    let expected = match header[2] {
        b'X' => b'X',
        b'A' => b'A',
        b'B' => b'B',
        _ => unreachable!(),
    };
    if payload.get(..2) != Some(&[expected, b'\n']) {
        return Err("invalid serialized workspace format".into());
    }
    Ok(payload)
}

#[derive(Debug)]
pub(super) struct Binding {
    pub(super) name: String,
    pub(super) symbol: Sexp<'static>,
    pub(super) value: Sexp<'static>,
}

pub(super) fn bindings(mut graph: Sexp<'static>) -> Result<Vec<Binding>, String> {
    let mut seen = hashbrown::HashSet::new();
    let mut result = Vec::new();
    while !graph.is_nil() {
        if graph.typeof_() != SEXPTYPE::LISTSXP {
            return Err("invalid serialized workspace binding list".into());
        }
        let identity = graph
            .allocation()
            .map_err(|e| e.to_string())?
            .link()
            .ok_or("invalid serialized workspace binding identity")?;
        seen.try_reserve(1).map_err(|_| allocation_error())?;
        if !seen.insert(identity) {
            return Err("cyclic serialized workspace binding list".into());
        }
        let symbol = graph
            .try_tag()
            .and_then(Sexp::into_owned)
            .map_err(|e| e.to_string())?;
        if symbol.typeof_() != SEXPTYPE::SYMSXP {
            return Err("invalid serialized workspace binding name".into());
        }
        let name = symbol
            .try_printname()
            .and_then(|s| s.try_as_string())
            .map_err(|e| e.to_string())?;
        if name.is_empty() {
            return Err("invalid serialized workspace binding name".into());
        }
        let value = graph
            .try_car()
            .and_then(Sexp::into_owned)
            .map_err(|e| e.to_string())?;
        result.try_reserve(1).map_err(|_| allocation_error())?;
        result.push(Binding {
            name,
            symbol,
            value,
        });
        graph = graph
            .try_cdr()
            .and_then(Sexp::into_owned)
            .map_err(|e| e.to_string())?;
    }
    Ok(result)
}

fn load_bytes_in(
    access: &RuntimeAccess,
    input: &[u8],
    environment: &Sexp<'static>,
) -> Result<Vec<String>, String> {
    access.require_active().map_err(|e| e.to_string())?;
    access
        .domain()
        .link(environment)
        .map_err(|e| e.to_string())?;
    let decompressed;
    let bytes = if input.starts_with(&[0x1f, 0x8b]) {
        decompressed = read(access, flate2::read::MultiGzDecoder::new(input))?;
        &decompressed.data
    } else if input.starts_with(b"BZh") {
        decompressed = read(access, bzip2::read::MultiBzDecoder::new(input))?;
        &decompressed.data
    } else if input.starts_with(&[0xfd, b'7', b'z', b'X', b'Z', 0]) {
        let budget = access
            .with_arena(|arena| arena.budget().max_bytes)
            .map_err(|e| e.to_string())?;
        let decoder = if budget == 0 {
            lzma_rust2::XzReader::new(input, true)
        } else {
            let kilobytes = budget.div_ceil(1024).min(u32::MAX as usize) as u32;
            lzma_rust2::XzReader::new_mem_limit(input, true, kilobytes)
        };
        decompressed = read(access, decoder)?;
        &decompressed.data
    } else {
        input
    };
    let graph = super::decode(access, payload(bytes)?).map_err(|e| e.to_string())?;
    let bindings = bindings(graph)?;
    let mut names = Vec::new();
    names
        .try_reserve_exact(bindings.len())
        .map_err(|_| allocation_error())?;
    // All remaining values own their roots independently of the decoded source
    // list. A setter can collect or mutate already-published bindings safely.
    for binding in &bindings {
        super::publish(access, environment, &binding.symbol, &binding.value)
            .map_err(|e| e.to_string())?;
        names.push(binding.name.clone());
    }
    Ok(names)
}

fn execute<T>(
    environment: &Sexp<'static>,
    operation: impl FnOnce(&RuntimeAccess) -> Result<T, String>,
) -> Result<T, String> {
    if environment.typeof_() != SEXPTYPE::ENVSXP {
        return Err("serialized data requires a target environment".into());
    }
    let authority = StoredOwner::from_value(environment).map_err(|e| e.to_string())?;
    let owner = authority
        .managed()
        .ok_or_else(|| SexpError::RootUnavailable.to_string())?;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        with_runtime(&owner, operation).map_err(|e| e.to_string())?
    }));
    authority.require_active().map_err(|e| e.to_string())?;
    match result {
        Ok(result) => result,
        Err(panic) => {
            if let Some(error) = panic.downcast_ref::<crate::sexp::context::RError>() {
                Err(error.message.clone())
            } else {
                std::panic::resume_unwind(panic)
            }
        }
    }
}

pub(super) fn load_file(path: &Path, environment: Sexp<'static>) -> Result<Vec<String>, String> {
    execute(&environment, |access| {
        let file = std::fs::File::open(path)
            .map_err(|e| format!("cannot read data file '{}': {e}", path.display()))?;
        let bytes = read(access, file)?;
        load_bytes_in(access, &bytes.data, &environment)
    })
}

pub(super) fn load_bytes(input: &[u8], environment: Sexp<'static>) -> Result<Vec<String>, String> {
    execute(&environment, |access| {
        load_bytes_in(access, input, &environment)
    })
}
