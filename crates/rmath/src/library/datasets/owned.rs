//! All lazy syntax, environments, inputs and results retain their original heap.
#![forbid(unsafe_code)]

use crate::sexp::{
    SEXPTYPE,
    object::{Sexp, SexpError, SexpMut, SexpResult},
    owner::{RuntimeAccess, StoredOwner, with_runtime},
};

#[cfg(test)]
#[path = "mutation_tests.rs"]
mod mutation_tests;

fn execute<T>(
    anchor: &Sexp<'static>,
    operation: impl FnOnce(&RuntimeAccess) -> Result<T, String>,
) -> Result<T, String> {
    let authority = StoredOwner::from_value(anchor).map_err(|e| e.to_string())?;
    let owner = authority
        .managed()
        .ok_or_else(|| SexpError::RootUnavailable.to_string())?;
    // Hold the original physical runtime through unwind and the post-callback check.
    let _pin = owner.pin().map_err(|e| e.to_string())?;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        with_runtime(&owner, operation).map_err(|e| e.to_string())?
    }));
    authority.require_active().map_err(|e| e.to_string())?;
    match result {
        Ok(result) => result,
        Err(payload) => {
            if let Some(error) = payload.downcast_ref::<crate::sexp::context::RError>() {
                Err(error.message.clone())
            } else {
                std::panic::resume_unwind(payload)
            }
        }
    }
}

fn integers(access: &RuntimeAccess, values: &[i32]) -> SexpResult<Sexp<'static>> {
    let domain = access.domain();
    let allocator = access.allocator(&domain)?;
    let length = values
        .len()
        .try_into()
        .map_err(|_| SexpError::AllocationFailed {
            object: "datasets key",
        })?;
    let result = allocator.allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, length)))?;
    let mut result = SexpMut::try_from_checked(result)?;
    for (index, value) in values.iter().copied().enumerate() {
        result.try_set_integer_elt(index as _, value)?;
    }
    Ok(result.freeze())
}

fn environment(access: &RuntimeAccess, parent: &Sexp<'static>) -> SexpResult<Sexp<'static>> {
    if parent.typeof_() != SEXPTYPE::ENVSXP {
        return Err(SexpError::TypeMismatch {
            expected: "environment",
            actual: parent.typeof_(),
        });
    }
    let domain = access.domain();
    let allocator = access.allocator(&domain)?;
    let nil = domain.nil();
    let body = crate::sexp::ffi::NodeBody::Environment(crate::sexp::ffi::Envsxp {
        frame: domain.link(&nil)?,
        enclos: domain.link(parent)?,
        hashtab: domain.link(&nil)?,
    });
    // Initialize every edge while the arena is exclusively lent. The producer
    // owns the result before collection/provider callbacks can run.
    allocator.allocate(|arena| {
        let raw = arena.alloc_node(SEXPTYPE::ENVSXP);
        let node = arena.node_token(raw)?;
        let heap = node.heap_identity();
        let mut header = heap.node_snapshot(&node)?;
        header.data = body;
        heap.replace_node(&node, header)?;
        Some(raw)
    })
}

fn namespace_in(access: &RuntimeAccess, base: &Sexp<'static>) -> SexpResult<Sexp<'static>> {
    if let Some(namespace) = super::cached(access)? {
        return Ok(namespace);
    }
    let domain = access.domain();
    let allocator = access.allocator(&domain)?;
    let base_namespace = super::base_namespace(access)?;
    let imports_environment = environment(access, &base_namespace)?;
    let imports_name = allocator.strings(&["imports:datasets"])?;
    super::set_attribute(access, &imports_environment, "name", &imports_name)?;
    let namespace = environment(access, &imports_environment)?;
    let info = environment(access, base)?;
    let exports = environment(access, base)?;
    let s3 = environment(access, base)?;
    let lazy = environment(access, base)?;
    let captured = environment(access, base)?;
    let file = allocator.strings(&[super::DATABASE])?;
    let compressed = integers(access, &[3])?;
    // The authenticated original database has no persistent environment references.
    // Keep the ordinary GNU hook operand in the captured frame nevertheless.
    let hook = domain.nil();
    super::bind(access, &captured, "datafile", &file)?;
    super::bind(access, &captured, "compressed", &compressed)?;
    super::bind(access, &captured, "envhook", &hook)?;
    let function = super::symbol(access, "lazyLoadDBfetch")?;
    let datafile_symbol = super::symbol(access, "datafile")?;
    let compressed_symbol = super::symbol(access, "compressed")?;
    let hook_symbol = super::symbol(access, "envhook")?;
    for &(name, offset, length) in super::inventory::OBJECTS {
        let key = integers(access, &[offset, length])?;
        let tail = allocator.pairlist_cell(&hook_symbol, &domain.nil(), &domain.nil())?;
        let tail = allocator.pairlist_cell(&compressed_symbol, &tail, &domain.nil())?;
        let tail = allocator.pairlist_cell(&datafile_symbol, &tail, &domain.nil())?;
        let tail = allocator.pairlist_cell(&key, &tail, &domain.nil())?;
        let expression = allocator.call(&function, &tail)?;
        let promise = allocator.promise(&expression, &captured)?;
        super::bind(access, &lazy, name, &promise)?;
    }
    let spec = allocator.strings(&["datasets", "4.7.0"])?;
    let spec_names = allocator.strings(&["name", "version"])?;
    super::set_attribute(access, &spec, "names", &spec_names)?;
    let path = allocator.strings(&[super::DIRECTORY])?;
    super::bind(access, &info, "exports", &exports)?;
    super::bind(access, &info, "spec", &spec)?;
    super::bind(access, &info, "path", &path)?;
    super::bind(access, &info, "lazydata", &lazy)?;
    let methods = allocator.allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::STRSXP, 0)))?;
    let dimensions = integers(access, &[0, 4])?;
    super::set_attribute(access, &methods, "dim", &dimensions)?;
    super::bind(access, &info, "S3methods", &methods)?;
    let empty_strings =
        allocator.allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::STRSXP, 0)))?;
    super::bind(access, &info, "dynlibs", &empty_strings)?;
    let native_routines =
        allocator.allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::VECSXP, 0)))?;
    super::bind(access, &info, "nativeRoutines", &native_routines)?;
    let imports = allocator.allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::VECSXP, 1)))?;
    let mut imports = SexpMut::try_from_checked(imports)?;
    imports.try_set_vector_elt(0, domain.logical(true))?;
    let imports = imports.freeze();
    let import_names = allocator.strings(&["base"])?;
    super::set_attribute(access, &imports, "names", &import_names)?;
    super::bind(access, &info, "imports", &imports)?;
    super::bind(access, &namespace, ".__NAMESPACE__.", &info)?;
    super::bind(access, &namespace, ".__S3MethodsTable__.", &s3)?;
    let package_name = allocator.strings(&["datasets"])?;
    super::bind(access, &namespace, ".packageName", &package_name)?;
    super::lock(access, &imports_environment)?;
    super::lock(access, &namespace)?;
    // Publication is last; callbacks cannot discover a partly constructed namespace.
    access.require_active()?;
    super::publish_namespace(access, &namespace)?;
    Ok(namespace)
}

fn lazy_in(access: &RuntimeAccess, base: &Sexp<'static>) -> SexpResult<Sexp<'static>> {
    let namespace = namespace_in(access, base)?;
    let info = super::lookup(access, &namespace, ".__NAMESPACE__.")?;
    super::lookup(access, &info, "lazydata")
}

pub(super) fn namespace(base: Sexp<'static>) -> Result<Sexp<'static>, String> {
    execute(&base, |access| {
        namespace_in(access, &base).map_err(|e| e.to_string())
    })
}

pub(super) fn index(base: Sexp<'static>) -> Result<Sexp<'static>, String> {
    execute(&base, |access| {
        let domain = access.domain();
        let allocator = access.allocator(&domain).map_err(|e| e.to_string())?;
        let rows = super::inventory::INDEX.len();
        let mut cells = Vec::new();
        cells
            .try_reserve_exact(rows * 4)
            .map_err(|_| "allocation failed while listing datasets")?;
        cells.extend(std::iter::repeat_n("datasets", rows));
        cells.extend(std::iter::repeat_n("<builtin>", rows));
        cells.extend(super::inventory::INDEX.iter().map(|(item, _)| *item));
        cells.extend(super::inventory::INDEX.iter().map(|(_, title)| *title));
        let matrix = allocator.strings(&cells).map_err(|e| e.to_string())?;
        let dimensions = integers(access, &[rows as i32, 4]).map_err(|e| e.to_string())?;
        super::set_attribute(access, &matrix, "dim", &dimensions).map_err(|e| e.to_string())?;
        let columns = allocator
            .strings(&["Package", "LibPath", "Item", "Title"])
            .map_err(|e| e.to_string())?;
        let dimnames = allocator
            .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::VECSXP, 2)))
            .map_err(|e| e.to_string())?;
        let mut dimnames = SexpMut::try_from_checked(dimnames).map_err(|e| e.to_string())?;
        dimnames
            .try_set_vector_elt(0, domain.nil())
            .map_err(|e| e.to_string())?;
        dimnames
            .try_set_vector_elt(1, columns)
            .map_err(|e| e.to_string())?;
        super::set_attribute(access, &matrix, "dimnames", &dimnames.freeze())
            .map_err(|e| e.to_string())?;
        let title = allocator
            .strings(&["Data sets"])
            .map_err(|e| e.to_string())?;
        let result = allocator
            .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::VECSXP, 4)))
            .map_err(|e| e.to_string())?;
        let mut result = SexpMut::try_from_checked(result).map_err(|e| e.to_string())?;
        result
            .try_set_vector_elt(0, title)
            .map_err(|e| e.to_string())?;
        result
            .try_set_vector_elt(1, domain.nil())
            .map_err(|e| e.to_string())?;
        result
            .try_set_vector_elt(2, matrix)
            .map_err(|e| e.to_string())?;
        result
            .try_set_vector_elt(3, domain.nil())
            .map_err(|e| e.to_string())?;
        let result = result.freeze();
        let names = allocator
            .strings(&["title", "header", "results", "footer"])
            .map_err(|e| e.to_string())?;
        super::set_attribute(access, &result, "names", &names).map_err(|e| e.to_string())?;
        let class = allocator
            .strings(&["packageIQR"])
            .map_err(|e| e.to_string())?;
        super::set_attribute(access, &result, "class", &class).map_err(|e| e.to_string())?;
        access.require_active().map_err(|e| e.to_string())?;
        Ok(result)
    })
}

pub(super) fn attach(base: Sexp<'static>) -> Result<(), String> {
    execute(&base, |access| {
        let lazy = lazy_in(access, &base).map_err(|e| e.to_string())?;
        let mut promises = Vec::new();
        promises
            .try_reserve_exact(super::inventory::OBJECTS.len())
            .map_err(|_| "allocation failed while attaching datasets")?;
        for &(name, _, _) in super::inventory::OBJECTS {
            promises.push((
                name,
                super::lookup(access, &lazy, name).map_err(|e| e.to_string())?,
            ));
        }
        let attached = environment(access, &base).map_err(|e| e.to_string())?;
        for (name, promise) in &promises {
            super::bind(access, &attached, name, promise).map_err(|e| e.to_string())?;
        }
        let domain = access.domain();
        let allocator = access.allocator(&domain).map_err(|e| e.to_string())?;
        let name = allocator
            .strings(&["package:datasets"])
            .map_err(|e| e.to_string())?;
        super::set_attribute(access, &attached, "name", &name).map_err(|e| e.to_string())?;
        let path = allocator
            .strings(&[super::DIRECTORY])
            .map_err(|e| e.to_string())?;
        super::set_attribute(access, &attached, "path", &path).map_err(|e| e.to_string())?;
        super::attach_environment(access, &attached).map_err(|e| e.to_string())
    })
}

pub(super) fn value(base: Sexp<'static>, name: &str) -> Result<Option<Sexp<'static>>, String> {
    if !super::inventory::OBJECTS
        .iter()
        .any(|(object, _, _)| *object == name)
    {
        return Ok(None);
    }
    execute(&base, |access| {
        let lazy = lazy_in(access, &base).map_err(|e| e.to_string())?;
        let promise = super::lookup(access, &lazy, name).map_err(|e| e.to_string())?;
        if promise == access.domain().unbound() {
            return Ok(None);
        }
        super::force(access, &promise)
            .map(Some)
            .map_err(|e| e.to_string())
    })
}

pub(super) fn load_topic(topic: &str, target: Sexp<'static>) -> Result<bool, String> {
    let Some((_, objects)) = super::inventory::TOPICS
        .iter()
        .find(|(name, _)| *name == topic)
    else {
        return Ok(false);
    };
    if target.typeof_() != SEXPTYPE::ENVSXP {
        return Err("invalid target environment".into());
    }
    execute(&target, |access| {
        let domain = access.domain();
        let allocator = access.allocator(&domain).map_err(|e| e.to_string())?;
        let file = allocator
            .strings(&[super::DATABASE])
            .map_err(|e| e.to_string())?;
        let compression = integers(access, &[3]).map_err(|e| e.to_string())?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(objects.len())
            .map_err(|_| "allocation failed while loading datasets")?;
        // data() reloads original records, independently of mutable namespace
        // lazydata bindings. Own every topic member before target setters run.
        for &name in *objects {
            let &(_, offset, size) = super::inventory::OBJECTS
                .iter()
                .find(|(object, _, _)| *object == name)
                .ok_or("invalid datasets topic inventory")?;
            let key = integers(access, &[offset, size]).map_err(|e| e.to_string())?;
            values.push((
                name,
                fetch(key, file.clone(), compression.clone(), domain.nil())?,
            ));
        }
        for (name, value) in &values {
            super::bind(access, &target, name, value).map_err(|e| e.to_string())?;
        }
        Ok(true)
    })
}

pub(super) fn read_database(
    file: Sexp<'static>,
    key: Sexp<'static>,
) -> Result<Option<Sexp<'static>>, String> {
    execute(&file, |access| {
        access.domain().link(&key).map_err(|e| e.to_string())?;
        if file.typeof_() != SEXPTYPE::STRSXP
            || file.len() != 1
            || !matches!(
                file.try_string_value_elt(0)
                    .map_err(|e| e.to_string())?
                    .as_deref(),
                Some(super::DATABASE) | Some(super::PACKAGE_DATABASE)
            )
        {
            return Ok(None);
        }
        access.require_active().map_err(|e| e.to_string())?;
        if key.typeof_() != SEXPTYPE::INTSXP || key.len() != 2 {
            return Err("bad offset/length argument".into());
        }
        let offset = key.try_integer_elt(0).map_err(|e| e.to_string())?;
        let length = key.try_integer_elt(1).map_err(|e| e.to_string())?;
        if !super::inventory::OBJECTS
            .iter()
            .any(|(_, start, count)| *start == offset && *count == length)
        {
            return Err("bad offset/length argument".into());
        }
        let bytes = include_bytes!("assets/Rdata.rdb");
        let chunk = bytes
            .get(offset as usize..(offset as usize + length as usize))
            .ok_or("bad offset/length argument")?;
        let domain = access.domain();
        let allocator = access.allocator(&domain).map_err(|e| e.to_string())?;
        let raw = allocator
            .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::RAWSXP, length.into())))
            .map_err(|e| e.to_string())?;
        let mut raw = SexpMut::try_from_checked(raw).map_err(|e| e.to_string())?;
        for (index, byte) in chunk.iter().copied().enumerate() {
            raw.try_set_raw_elt(index as _, byte)
                .map_err(|e| e.to_string())?;
        }
        access.require_active().map_err(|e| e.to_string())?;
        Ok(Some(raw.freeze()))
    })
}

pub(super) fn fetch(
    key: Sexp<'static>,
    file: Sexp<'static>,
    compressed: Sexp<'static>,
    hook: Sexp<'static>,
) -> Result<Sexp<'static>, String> {
    execute(&file, |access| {
        let domain = access.domain();
        domain.link(&key).map_err(|e| e.to_string())?;
        domain.link(&compressed).map_err(|e| e.to_string())?;
        domain.link(&hook).map_err(|e| e.to_string())?;
        let compression = super::compression(access, &compressed).map_err(|e| e.to_string())?;
        if compression != 3 {
            return Err(format!(
                "lazy-load database '{}' is corrupt",
                super::DATABASE
            ));
        }
        let raw =
            read_database(file.clone(), key.clone())?.ok_or("not a portable datasets database")?;
        // The original, authenticated records bound both the input and expanded
        // buffer size; reserve that native-provider workspace before decoding.
        // Read the private selected bytes, never reread a caller's mutable key
        // after the raw-vector allocation can run a collection callback.
        let mut prefix = [0; 4];
        for (index, byte) in prefix.iter_mut().enumerate() {
            *byte = raw.try_raw_elt(index as _).map_err(|e| e.to_string())?;
        }
        let expanded = u32::from_be_bytes(prefix) as usize;
        let workspace = expanded
            .checked_add(
                raw.len()
                    .try_into()
                    .map_err(|_| "datasets record is too large")?,
            )
            .ok_or("datasets record is too large")?;
        let _reservation = access
            .with_arena(|arena| arena.try_reserve_transient(workspace))
            .map_err(|e| e.to_string())?
            .ok_or("allocation failed while decoding datasets")?;
        let decoded = decompress(access, &raw, expanded)?;
        let value = super::decode(access, &decoded, &hook).map_err(|e| e.to_string())?;
        let value = super::force(access, &value).map_err(|e| e.to_string())?;
        access.require_active().map_err(|e| e.to_string())?;
        Ok(value)
    })
}

fn decompress(
    access: &RuntimeAccess,
    raw: &Sexp<'static>,
    expanded: usize,
) -> Result<Sexp<'static>, String> {
    use std::io::Write;

    // Every admitted key selects an authenticated, immutable compression-3/Z
    // record. Copy it before decoding, without lending an R payload to a codec.
    let length: usize = raw
        .len()
        .try_into()
        .map_err(|_| "datasets record is too large")?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| "allocation failed while decoding datasets")?;
    for index in 0..length {
        bytes.push(raw.try_raw_elt(index as _).map_err(|e| e.to_string())?);
    }
    if bytes.get(4) != Some(&b'Z') {
        return Err(format!(
            "lazy-load database '{}' is corrupt",
            super::DATABASE
        ));
    }

    struct BoundedOutput {
        bytes: Vec<u8>,
        limit: usize,
    }
    impl Write for BoundedOutput {
        fn write(&mut self, input: &[u8]) -> std::io::Result<usize> {
            if input.len() > self.limit.saturating_sub(self.bytes.len()) {
                return Err(std::io::Error::other(
                    "datasets decoded length exceeds its original record",
                ));
            }
            self.bytes.extend_from_slice(input);
            Ok(input.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut output = BoundedOutput {
        bytes: Vec::new(),
        limit: expanded,
    };
    output
        .bytes
        .try_reserve_exact(expanded)
        .map_err(|_| "allocation failed while decoding datasets")?;
    lzma_rs::lzma2_decompress(&mut std::io::BufReader::new(&bytes[5..]), &mut output)
        .map_err(|_| format!("lazy-load database '{}' is corrupt", super::DATABASE))?;
    if output.bytes.len() != expanded {
        return Err(format!(
            "lazy-load database '{}' is corrupt",
            super::DATABASE
        ));
    }
    access.require_active().map_err(|e| e.to_string())?;
    let domain = access.domain();
    let allocator = access.allocator(&domain).map_err(|e| e.to_string())?;
    let length = expanded
        .try_into()
        .map_err(|_| "datasets record is too large")?;
    let decoded = allocator
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::RAWSXP, length)))
        .map_err(|e| e.to_string())?;
    let mut decoded = SexpMut::try_from_checked(decoded).map_err(|e| e.to_string())?;
    for (index, byte) in output.bytes.into_iter().enumerate() {
        decoded
            .try_set_raw_elt(index as _, byte)
            .map_err(|e| e.to_string())?;
    }
    access.require_active().map_err(|e| e.to_string())?;
    Ok(decoded.freeze())
}
