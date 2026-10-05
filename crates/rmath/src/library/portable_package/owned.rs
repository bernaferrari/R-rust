//! Original package operands and decoded graphs own their original heap.
#![forbid(unsafe_code)]
use super::{LazyDatabase, PackageImage};
use crate::sexp::{
    SEXPTYPE,
    object::{Sexp, SexpError, SexpMut, SexpResult},
    owner::{RuntimeAccess, StoredOwner, with_runtime},
};
use std::io::Read;

fn failure(message: impl Into<String>) -> SexpError {
    SexpError::EvaluationFailed {
        message: message.into(),
    }
}

fn execute<T>(
    anchor: &Sexp<'static>,
    operation: impl FnOnce(&RuntimeAccess) -> SexpResult<T>,
) -> Result<T, String> {
    let authority = StoredOwner::from_value(anchor).map_err(|e| e.to_string())?;
    let owner = authority
        .managed()
        .ok_or_else(|| SexpError::RootUnavailable.to_string())?;
    let _pin = owner.pin().map_err(|e| e.to_string())?;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        with_runtime(&owner, operation)
            .and_then(|result| result)
            .map_err(|e| e.to_string())
    }));
    authority.require_active().map_err(|e| e.to_string())?;
    match result {
        Ok(result) => result,
        Err(payload) => {
            if let Some(error) = payload.downcast_ref::<crate::sexp::context::RError>() {
                Err(error.message.clone())
            } else if let Some(crate::sexp::context::RSignal::Error { message }) =
                payload.downcast_ref::<crate::sexp::context::RSignal>()
            {
                Err(message.clone())
            } else {
                std::panic::resume_unwind(payload)
            }
        }
    }
}

fn raw(access: &RuntimeAccess, bytes: &[u8]) -> SexpResult<Sexp<'static>> {
    let domain = access.domain();
    let allocator = access.allocator(&domain)?;
    let length = bytes
        .len()
        .try_into()
        .map_err(|_| SexpError::AllocationFailed {
            object: "portable package serialized record",
        })?;
    let value = allocator.allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::RAWSXP, length)))?;
    let mut value = SexpMut::try_from_checked(value)?;
    for (index, byte) in bytes.iter().copied().enumerate() {
        value.try_set_raw_elt(index as _, byte)?;
    }
    Ok(value.freeze())
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
    allocator.allocate(|arena| {
        let pointer = arena.alloc_node(SEXPTYPE::ENVSXP);
        let node = arena.node_token(pointer)?;
        let heap = node.heap_identity();
        let mut header = heap.node_snapshot(&node)?;
        header.data = body;
        heap.replace_node(&node, header)?;
        Some(pointer)
    })
}

fn key_range(
    image: &LazyDatabase,
    access: &RuntimeAccess,
    key: &Sexp<'static>,
) -> SexpResult<std::ops::Range<usize>> {
    access.domain().link(key)?;
    if key.typeof_() != SEXPTYPE::INTSXP {
        return Err(failure("bad offset/length argument"));
    }
    let length = key.len();
    access.require_active()?;
    if length != 2 {
        return Err(failure("bad offset/length argument"));
    }
    let offset = key.try_integer_elt(0)?;
    access.require_active()?;
    let length = key.try_integer_elt(1)?;
    access.require_active()?;
    let offset = usize::try_from(offset).map_err(|_| failure("bad offset/length argument"))?;
    let length = usize::try_from(length).map_err(|_| failure("bad offset/length argument"))?;
    let end = offset
        .checked_add(length)
        .filter(|end| *end <= image.bytes.len())
        .ok_or_else(|| failure("read failed"))?;
    Ok(offset..end)
}

pub(super) fn read_database(
    image: &'static LazyDatabase,
    file: Sexp<'static>,
    key: Sexp<'static>,
) -> Result<Option<Sexp<'static>>, String> {
    execute(&file, |access| {
        if !image.is_database(&file)? {
            return Ok(None);
        }
        let range = key_range(image, access, &key)?;
        raw(access, &image.bytes[range]).map(Some)
    })
}

pub(super) fn fetch(
    image: &'static LazyDatabase,
    key: Sexp<'static>,
    file: Sexp<'static>,
    compressed: Sexp<'static>,
    hook: Sexp<'static>,
) -> Result<Sexp<'static>, String> {
    execute(&file, |access| {
        let domain = access.domain();
        domain.link(&key)?;
        domain.link(&compressed)?;
        domain.link(&hook)?;
        if !image.is_database(&file)? {
            return Err(failure("not the portable package database"));
        }
        if compressed.typeof_() != SEXPTYPE::INTSXP && compressed.typeof_() != SEXPTYPE::LGLSXP {
            return Err(failure("portable package database compression is invalid"));
        }
        let length = compressed.len();
        access.require_active()?;
        if length != 1 {
            return Err(failure("portable package database compression is invalid"));
        }
        let compressed_value = if compressed.typeof_() == SEXPTYPE::INTSXP {
            compressed.try_integer_elt(0)?
        } else {
            compressed.try_logical_elt(0)?
        };
        access.require_active()?;
        if compressed_value != 1 {
            return Err(failure("portable package database compression is invalid"));
        }
        let range = key_range(image, access, &key)?;
        let bytes = &image.bytes[range];
        let prefix: [u8; 4] = bytes
            .get(..4)
            .ok_or_else(|| failure("portable package database is corrupt"))?
            .try_into()
            .map_err(|_| failure("portable package database is corrupt"))?;
        let expected = u32::from_be_bytes(prefix) as usize;
        let capacity = expected
            .checked_add(1)
            .ok_or_else(|| failure("portable package record is too large"))?;
        let workspace = capacity
            .checked_add(bytes.len())
            .ok_or_else(|| failure("portable package record is too large"))?;
        let _reservation = access
            .with_arena(|arena| arena.try_reserve_transient(workspace))?
            .ok_or(SexpError::AllocationFailed {
                object: "portable package decompression workspace",
            })?;
        let mut decoded = Vec::new();
        decoded
            .try_reserve_exact(capacity)
            .map_err(|_| SexpError::AllocationFailed {
                object: "portable package decompression workspace",
            })?;
        flate2::read::ZlibDecoder::new(&bytes[4..])
            .take(capacity as u64)
            .read_to_end(&mut decoded)
            .map_err(|_| failure("portable package database is corrupt"))?;
        if decoded.len() != expected {
            return Err(failure("portable package database is corrupt"));
        }
        let raw = raw(access, &decoded)?;
        let result = super::bridge::decode(access, &raw, &hook)?;
        let result = super::bridge::force(access, &result)?;
        if image.path == "<builtin:methods>/R/methods.rdb" {
            super::bridge::restore_methods_metadata(access, &result)?;
        }
        access.require_active()?;
        Ok(result)
    })
}

fn original_object(access: &RuntimeAccess, bytes: &[u8]) -> SexpResult<Sexp<'static>> {
    let input = raw(access, bytes)?;
    super::bridge::decode(access, &input, &access.domain().nil())
}

fn install_lazy_database(
    access: &RuntimeAccess,
    database: &LazyDatabase,
    envhook: &str,
    namespace: &Sexp<'static>,
    base: &Sexp<'static>,
    exported: &Sexp<'static>,
) -> SexpResult<()> {
    let setup = environment(access, base)?;
    let index = original_object(access, database.index)?;
    let allocator = access.allocator(&access.domain())?;
    super::bridge::bind(access, &setup, "namespace", &namespace)?;
    super::bridge::bind(access, &setup, "map", &index)?;
    super::bridge::bind(access, &setup, "exported", &exported)?;
    let datafile = allocator.strings(&[database.path])?;
    super::bridge::bind(access, &setup, "datafile", &datafile)?;
    let hook_source = format!("envhook <- {}", envhook);
    super::bridge::evaluate(
        access,
        r#"
        existsInFrame <- function(x, env) .Internal(exists(x, env, "any", FALSE))
        mkenv <- function() .Internal(new.env(TRUE, baseenv(), 29L))
        environment <- function() .Internal(environment(NULL))
        list2env <- function(x, envir) .Internal(list2env(x, envir))
        `parent.env<-` <- function(env, value) .Internal(`parent.env<-`(env, value))
        env <- mkenv(); list2env(map$references, env)
        envenv <- mkenv(); compressed <- map$compressed
    "#,
        &setup,
    )
    .map_err(failure)?;
    super::bridge::evaluate(access, &hook_source, &setup).map_err(failure)?;
    super::bridge::evaluate(access, r#"
        expr <- quote(lazyLoadDBfetch(KEY, datafile, compressed, envhook))
        .Internal(makeLazy(names(map$variables), map$variables, expr, environment(), namespace))
        for (name in exported) assign(name, TRUE, envir=get("exports", get(".__NAMESPACE__.", namespace)))
        map <- NULL; exported <- NULL
    "#, &setup).map_err(failure)?;
    Ok(())
}

fn namespace_in(
    image: &'static PackageImage,
    access: &RuntimeAccess,
    base: &Sexp<'static>,
) -> SexpResult<Sexp<'static>> {
    if let Some(namespace) = super::bridge::cached(access, image)? {
        return Ok(namespace);
    }
    let domain = access.domain();
    let allocator = access.allocator(&domain)?;
    let base_namespace = super::bridge::base_namespace(access)?;
    let imports_environment = environment(access, &base_namespace)?;
    let imports_name = allocator.strings(&[&format!("imports:{}", image.name)])?;
    super::bridge::set_attribute(access, &imports_environment, "name", &imports_name)?;
    let namespace = environment(access, &imports_environment)?;
    let info = environment(access, base)?;
    let exports = environment(access, base)?;
    let s3 = environment(access, base)?;
    let empty_lazy = environment(access, base)?;
    let exported = original_object(access, image.exports)?;
    let s3methods = original_object(access, image.s3_methods)?;
    let spec = allocator.strings(&[image.name, image.version])?;
    let spec_names = allocator.strings(&["name", "version"])?;
    super::bridge::set_attribute(access, &spec, "names", &spec_names)?;
    let path = allocator.strings(&[image.directory])?;
    super::bridge::bind(access, &info, "spec", &spec)?;
    super::bridge::bind(access, &info, "path", &path)?;
    super::bridge::bind(access, &info, "exports", &exports)?;
    super::bridge::bind(access, &info, "lazydata", &empty_lazy)?;
    super::bridge::bind(access, &info, "S3methods", &s3methods)?;
    // GNU's pinned NAMESPACE has no import directives; its actual imports is base=TRUE.
    let imports = allocator.allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::VECSXP, 1)))?;
    let mut imports = SexpMut::try_from_checked(imports)?;
    imports.try_set_vector_elt(0, domain.logical(true))?;
    let imports = imports.freeze();
    let import_names = allocator.strings(&["base"])?;
    super::bridge::set_attribute(access, &imports, "names", &import_names)?;
    super::bridge::bind(access, &info, "imports", &imports)?;
    super::bridge::bind(access, &namespace, ".__NAMESPACE__.", &info)?;
    super::bridge::bind(access, &namespace, ".__S3MethodsTable__.", &s3)?;
    let package = allocator.strings(&[image.name])?;
    super::bridge::bind(access, &namespace, ".packageName", &package)?;
    install_lazy_database(
        access,
        &image.database,
        image.envhook,
        &namespace,
        base,
        &exported,
    )?;
    if let Some(database) = &image.sysdata {
        install_lazy_database(
            access,
            database,
            image.envhook,
            &namespace,
            base,
            &domain.nil(),
        )?;
    }
    super::bridge::lock(access, &imports_environment)?;
    // The complete original lazy bindings and metadata exist before publication.
    // Namespace references/onLoad legitimately resolve the in-flight identity.
    let publication = super::bridge::Publication::begin(access, image, &namespace)?;
    super::bridge::install_native(access, image, &namespace)?;
    super::bridge::finalize(access, image, &namespace)?;
    access.require_active()?;
    super::bridge::lock(access, &namespace)?;
    publication.commit(access)?;
    Ok(namespace)
}

pub(super) fn namespace(
    image: &'static PackageImage,
    base: Sexp<'static>,
) -> Result<Sexp<'static>, String> {
    execute(&base, |access| namespace_in(image, access, &base))
}

pub(super) fn attach(image: &'static PackageImage, base: Sexp<'static>) -> Result<(), String> {
    execute(&base, |access| {
        let namespace = namespace_in(image, access, &base)?;
        let exported = original_object(access, image.exports)?;
        let length = exported.len();
        access.require_active()?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(length as usize)
            .map_err(|_| SexpError::AllocationFailed {
                object: "portable package exports",
            })?;
        for index in 0..length {
            let name = exported.try_string_elt(index)?.try_as_string()?;
            values.push((
                name.clone(),
                super::bridge::lookup(access, &namespace, &name)?,
            ));
        }
        let attached = environment(access, &base)?;
        for (name, value) in values {
            super::bridge::bind(access, &attached, &name, &value)?;
        }
        let package_name = access
            .allocator(&access.domain())?
            .strings(&[&format!("package:{}", image.name)])?;
        super::bridge::set_attribute(access, &attached, "name", &package_name)?;
        let name = access.allocator(&access.domain())?.strings(&[image.name])?;
        super::bridge::bind(access, &attached, ".packageName", &name)?;
        super::bridge::bind(access, &attached, ".namespaceEnv", &namespace)?;
        super::bridge::lock(access, &attached)?;
        super::bridge::attach(access, &attached)?;
        access.require_active()?;
        Ok(())
    })
}
