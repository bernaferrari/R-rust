use super::*;

// ---------------------------------------------------------------------------
// R_serialize / R_unserialize (R-level entry points, memory-based)
// ---------------------------------------------------------------------------

/// Serialize an R object to a raw vector (when icon is R_NilValue).
/// This is the main entry point for `serialize()` in R.
pub fn R_serialize(object: SEXP, icon: SEXP, ascii: SEXP, Sversion: SEXP, fun: SEXP) -> SEXP {
    R_serialize_with_xdr(object, icon, ascii, R_NilValue(), Sversion, fun)
}

pub fn R_serialize_with_xdr(
    object: SEXP,
    icon: SEXP,
    ascii: SEXP,
    xdr: SEXP,
    Sversion: SEXP,
    fun: SEXP,
) -> SEXP {
    {
        if object.is_null() {
            error("read error");
        }

        let owner = current_owner();
        let _pin = owner.pin().unwrap_or_else(|e| error(&e.to_string()));
        let _object_guard = protect(object);
        let _hook_guard = protect(fun);
        let version = if Sversion == R_NilValue() {
            defaultSerializeVersion()
        } else {
            let version = asInteger(Sversion);
            if version != 2 && version != 3 {
                error(&format!("version {version} not supported"));
            }
            version
        };

        let ascii_value = if ascii.is_null() || ascii == R_NilValue() {
            0
        } else {
            asLogical(ascii)
        };
        let format = if ascii_value != 0 {
            SnapshotFormat::Ascii {
                hex: ascii_value == crate::sexp::ffi::NA_LOGICAL,
            }
        } else if xdr.is_null() || xdr == R_NilValue() || asLogical(xdr) != 0 {
            SnapshotFormat::Xdr
        } else {
            SnapshotFormat::Native
        };
        let data = serialize_snapshot(&owner, object, fun, version, format, &[]);
        let raw = raw_from_bytes(&data);
        if raw.is_null() {
            error("serialize exceeds session memory limit");
        }
        raw
    }
}

/// Encode a complete, arena-admitted workspace before publishing its file.
pub(crate) fn serialize_workspace_snapshot(
    owner: &crate::sexp::owner::OwnerToken<'_>,
    bindings: SEXP,
    version: c_int,
    ascii: bool,
) -> SerializedBytes {
    let prefix: &[u8] = match (version, ascii) {
        (2, false) => b"RDX2\n",
        (3, false) => b"RDX3\n",
        (2, true) => b"RDA2\n",
        (3, true) => b"RDA3\n",
        _ => error(&format!("version {version} not supported")),
    };
    {
        let format = if ascii {
            SnapshotFormat::Ascii { hex: false }
        } else {
            SnapshotFormat::Xdr
        };
        serialize_snapshot(owner, bindings, R_NilValue(), version, format, prefix)
    }
}

enum SnapshotFormat {
    Ascii { hex: bool },
    Xdr,
    Native,
}

fn serialize_snapshot(
    owner: &crate::sexp::owner::OwnerToken<'_>,
    object: SEXP,
    hook: SEXP,
    version: c_int,
    format: SnapshotFormat,
    prefix: &[u8],
) -> SerializedBytes {
    let _object_guard = protect(object);
    let _hook_guard = protect(hook);
    // Build the header
    let mut writer = BinaryWriter::admitted(owner);
    writer.strict_workspace = !prefix.is_empty();
    writer.write_bytes(prefix);
    writer.set_persist_hook(hook);
    writer.write_byte(match format {
        SnapshotFormat::Ascii { .. } => b'A',
        SnapshotFormat::Xdr => b'X',
        SnapshotFormat::Native => b'B',
    });
    writer.write_byte(b'\n');
    writer.set_ascii_body(matches!(format, SnapshotFormat::Ascii { .. }));
    writer.set_ascii_hex(matches!(format, SnapshotFormat::Ascii { hex: true }));
    writer.set_xdr_body(matches!(format, SnapshotFormat::Xdr));
    writer.version = version;

    // Version info
    writer.write_i32(version); // version
    writer.write_i32(R_VERSION); // writer version
    writer.write_i32(if version == 2 {
        R_VERSION_230
    } else {
        R_VERSION_350
    });
    if version == 3 {
        // GNU R_Serialize writes the native encoding name; the runtime
        // is UTF-8-native like Unix GNU R.
        const NATIVE_ENCODING: &[u8] = b"UTF-8";
        writer.write_i32(NATIVE_ENCODING.len() as c_int);
        writer.write_string_bytes(NATIVE_ENCODING);
    }

    // Serialize the object
    let mut ref_table = WriteHashTable::for_owner(owner);
    WriteItemInternal(object, &mut ref_table, &mut writer);

    owner
        .require_active()
        .unwrap_or_else(|e| error(&e.to_string()));
    writer.into_output()
}

/// Unserialize an R object from a raw vector.
pub fn R_unserialize(icon: SEXP, fun: SEXP) -> SEXP {
    {
        if !fun.is_null() && fun != R_NilValue() && TYPEOF(fun) == SEXPTYPE::ENVSXP {
            let owner = current_owner();
            let _pin = owner.pin().unwrap_or_else(|e| error(&e.to_string()));
            owner
                .require_active()
                .unwrap_or_else(|e| error(&e.to_string()));
            let data = owner
                .sexp(fun)
                .and_then(|s| s.into_owned())
                .unwrap_or_else(|e| error(&e.to_string()));
            unserialize_raw(icon, R_NilValue(), Some(LazyLoadRestore { data }))
        } else {
            unserialize_raw(icon, fun, None)
        }
    }
}

/// The runtime's lazy-load hook is Rust control flow with an actual owning
/// environment.
#[derive(Clone)]
pub(crate) struct LazyLoadRestore {
    data: crate::sexp::object::Sexp<'static>,
}

impl LazyLoadRestore {
    pub(super) fn restore(&self, names: SEXP) -> Result<SEXP, String> {
        use crate::sexp::{
            object::SexpError,
            owner::{OwnerToken, StoredOwner},
        };
        let authority = StoredOwner::from_value(&self.data).map_err(|e| e.to_string())?;
        authority
            .with_projection(|pointer| {
                // This original operation pin retains cleanup storage through unwind.
                // SAFETY: `pointer` is the projected owner of the stored
                // authority, valid for the duration of the projection.
                let owner = unsafe { OwnerToken::from_raw(pointer) };
                owner.require_active()?;
                if crate::sexp::memory::is_arena_lent(pointer) {
                    return Err(SexpError::OwnerNotActive);
                }
                let names = owner.sexp(names)?;
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                    // SAFETY: `pointer` is the projected owner whose instance
                    // is activated only inside this pinned callback scope.
                    crate::sexp::session::with_instance_active(pointer, || {
                        let raw = persist_restore_inner(names.as_raw(), self.data.as_raw(), owner);
                        owner.require_active()?;
                        owner.sexp(raw)?.into_owned()
                    })
                }));
                // Revoked authority cannot publish a value or a callback panic.
                owner.require_active()?;
                match result {
                    Ok(value) => value.map(|value| value.as_raw()),
                    Err(payload) => std::panic::resume_unwind(payload),
                }
            })
            .map_err(|e| e.to_string())
    }
}

/// A failed reconstruction must not leave its provisional environment in the
/// shared reference cache. Cleanup changes only this original, exact binding;
/// it neither looks up an ambient owner nor invokes active binding callbacks.
struct PendingPersistEnvironment<'s> {
    cache: crate::sexp::object::Sexp<'s>,
    symbol: crate::sexp::object::Sexp<'s>,
    environment: crate::sexp::object::Sexp<'s>,
    owner: crate::sexp::owner::OwnerToken<'s>,
    _pin: Option<crate::sexp::owner::OwnerPin>,
    committed: bool,
}
impl Drop for PendingPersistEnvironment<'_> {
    fn drop(&mut self) {
        use crate::sexp::{
            ffi::{EdgeField, NodeBody},
            heap::ResolvedLink,
        };
        if self.committed {
            return;
        }
        let Ok(cache) = self.cache.allocation() else {
            return;
        };
        let heap = cache.heap_identity();
        let Some(symbol) = self.symbol.allocation().ok().and_then(|n| n.link()) else {
            return;
        };
        let Some(environment) = self.environment.allocation().ok().and_then(|n| n.link()) else {
            return;
        };
        let Some(header) = heap.node_snapshot(cache) else {
            return;
        };
        let NodeBody::Environment(body) = header.data else {
            return;
        };
        let mut current = body.frame;
        let mut previous = None;
        let mut seen = std::collections::HashSet::new();
        while seen.insert(current) {
            let Some(ResolvedLink::Node { allocation, .. }) = heap.resolve_link(current) else {
                return;
            };
            let Some(header) = heap.node_snapshot(&allocation) else {
                return;
            };
            let NodeBody::List(cell) = header.data else {
                return;
            };
            if cell.tagval == symbol && cell.carval == environment {
                let (parent, field) = previous
                    .map_or((cache.clone(), EdgeField::EnvironmentFrame), |p| {
                        (p, EdgeField::ListCdr)
                    });
                let Some(mut header) = heap.node_snapshot(&parent) else {
                    return;
                };
                let Some(next) = heap.projection_of_link(cell.cdrval) else {
                    return;
                };
                let Some(parent_pointer) = heap.node_projection(&parent) else {
                    return;
                };
                // SAFETY: both canonical nodes and the physical runtime are
                // retained; this barrier is callback-free and remains valid
                // after revocation.
                if !unsafe {
                    crate::sexp::gengc::write_barrier_in(self.owner.as_ptr(), parent_pointer, next)
                } {
                    return;
                }
                if header.set_edge(field, cell.cdrval).is_some() {
                    heap.replace_node(&parent, header);
                }
                return;
            }
            previous = Some(allocation);
            current = cell.cdrval;
        }
    }
}

fn persist_restore_inner(
    names: SEXP,
    data: SEXP,
    owner: crate::sexp::owner::OwnerToken<'_>,
) -> SEXP {
    {
        if data.is_null() || TYPEOF(data) != SEXPTYPE::ENVSXP {
            return R_NilValue();
        }
        let name = if TYPEOF(names) == SEXPTYPE::STRSXP && XLENGTH(names) > 0 {
            let elt = STRING_ELT(names, 0);
            if elt.is_null() {
                String::new()
            } else {
                String::from_utf8_lossy(&charsxp_as_utf8(elt)).into_owned()
            }
        } else {
            String::new()
        };
        if name.is_empty() {
            return R_NilValue();
        }
        let name_sym = install_str(&name);

        let cache_sym = install_str("cache");
        let cache = R_findVarInFrame(data, cache_sym);
        if !cache.is_null() && cache != R_UnboundValue() && TYPEOF(cache) == SEXPTYPE::ENVSXP {
            let hit = R_findVarInFrame(cache, name_sym);
            if !hit.is_null() && hit != R_UnboundValue() {
                return hit;
            }
        }
        // GNU package/namespace persist: reuse the live namespace so
        // lazy-loaded closures keep `standardGeneric` and other ns bindings.
        let package = name.strip_prefix("package:").unwrap_or(name.as_str());
        if let Some(live) = crate::mainutils::essentials::cached_namespace_by_name(package) {
            if !cache.is_null() && cache != R_UnboundValue() && TYPEOF(cache) == SEXPTYPE::ENVSXP {
                crate::sexp::envir::defineVar(name_sym, live, cache);
            }
            return live;
        }

        let factory = owner.node_factory();
        let environment = factory
            .wrap(crate::sexp::memory_ext::NewEnvironment(
                R_NilValue(),
                R_EmptyEnv(),
                R_NilValue(),
            ))
            .unwrap_or_else(|e| error(&e.to_string()));
        let env_raw = environment.as_raw();
        let mut pending =
            if !cache.is_null() && cache != R_UnboundValue() && TYPEOF(cache) == SEXPTYPE::ENVSXP {
                let cache = factory
                    .wrap(cache)
                    .unwrap_or_else(|e| error(&e.to_string()));
                let symbol = factory
                    .wrap(name_sym)
                    .unwrap_or_else(|e| error(&e.to_string()));
                Some(PendingPersistEnvironment {
                    cache,
                    symbol,
                    environment: environment.clone(),
                    owner,
                    _pin: owner.pin().unwrap_or_else(|e| error(&e.to_string())),
                    committed: false,
                })
            } else {
                None
            };
        if let Some(pending) = &pending {
            crate::sexp::envir::define_var_safe(
                pending.symbol.clone(),
                environment.clone(),
                pending.cache.clone(),
            );
        }
        let env = env_raw;
        let refs_sym = install_str("refs");
        let refs = R_findVarInFrame(data, refs_sym);
        let mut key = R_NilValue();
        if !refs.is_null() && refs != R_UnboundValue() {
            if TYPEOF(refs) == SEXPTYPE::ENVSXP {
                key = R_findVarInFrame(refs, name_sym);
            } else if TYPEOF(refs) == SEXPTYPE::VECSXP {
                let nm = crate::sexp::attrib_core::getAttrib(
                    refs,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                );
                if TYPEOF(nm) == SEXPTYPE::STRSXP {
                    for i in 0..XLENGTH(nm) {
                        let raw = STRING_ELT(nm, i);
                        if !raw.is_null()
                            && String::from_utf8_lossy(&charsxp_as_utf8(raw)) == name.as_str()
                        {
                            key = VECTOR_ELT(refs, i);
                            break;
                        }
                    }
                }
            }
        }
        if key.is_null() || key == R_UnboundValue() || key == R_NilValue() {
            error(&format!("lazy-load reference '{name}' has no payload"));
        }
        if TYPEOF(key) == SEXPTYPE::VECSXP {
            let kn =
                crate::sexp::attrib_core::getAttrib(key, crate::sexp::attrib_core::R_NamesSymbol());
            if TYPEOF(kn) == SEXPTYPE::STRSXP {
                for i in 0..XLENGTH(kn) {
                    let raw = STRING_ELT(kn, i);
                    if !raw.is_null()
                        && String::from_utf8_lossy(&charsxp_as_utf8(raw)) == "eagerKey"
                    {
                        key = VECTOR_ELT(key, i);
                        break;
                    }
                }
            }
        }
        let datafile = R_findVarInFrame(data, install_str("datafile"));
        let compressed = R_findVarInFrame(data, install_str("compressed"));
        if datafile.is_null()
            || datafile == R_UnboundValue()
            || compressed.is_null()
            || compressed == R_UnboundValue()
        {
            error("lazy-load reference lacks its database path or compression mode");
        }
        let fields = [key, datafile, compressed, data]
            .map(|raw| factory.wrap(raw).unwrap_or_else(|e| error(&e.to_string())));
        let mut args = factory.nil();
        for field in fields.iter().rev() {
            args = factory
                .pairlist_cell(field, &args, &factory.nil())
                .unwrap_or_else(|e| error(&e.to_string()));
        }
        let fetched = factory
            .wrap(do_lazyLoadDBfetch(
                R_NilValue(),
                R_NilValue(),
                args.as_raw(),
                R_NilValue(),
            ))
            .unwrap_or_else(|e| error(&e.to_string()));
        owner
            .require_active()
            .unwrap_or_else(|e| error(&e.to_string()));
        let fetched_raw = fetched.as_raw();
        if TYPEOF(fetched_raw) != SEXPTYPE::VECSXP {
            error("invalid lazy-load environment payload");
        }
        let fetched = fetched_raw;
        if TYPEOF(fetched) == SEXPTYPE::VECSXP {
            let fnames = crate::sexp::attrib_core::getAttrib(
                fetched,
                crate::sexp::attrib_core::R_NamesSymbol(),
            );
            if TYPEOF(fnames) != SEXPTYPE::STRSXP {
                error("lazy-load environment payload has no field names");
            }
            let mut fields = Vec::new();
            for i in 0..XLENGTH(fnames) {
                let label = factory
                    .wrap(STRING_ELT(fnames, i))
                    .unwrap_or_else(|e| error(&e.to_string()))
                    .as_string()
                    .unwrap_or_default();
                let value = factory
                    .wrap(VECTOR_ELT(fetched, i))
                    .unwrap_or_else(|e| error(&e.to_string()));
                fields.push((label, value));
            }
            {
                for (label, value) in fields {
                    let elt = value.as_raw();
                    match label.as_str() {
                        "enclos" => {
                            if TYPEOF(elt) != SEXPTYPE::ENVSXP {
                                error("invalid lazy-load environment enclosure");
                            }
                            crate::sexp::accessors::SET_ENCLOS(env, elt);
                        }
                        "bindings" => {
                            if TYPEOF(elt) != SEXPTYPE::VECSXP {
                                error("invalid lazy-load environment bindings");
                            }
                            let tail = factory
                                .pairlist_cell(&environment, &factory.nil(), &factory.nil())
                                .unwrap_or_else(|e| error(&e.to_string()));
                            let call = factory
                                .pairlist_cell(&value, &tail, &factory.nil())
                                .unwrap_or_else(|e| error(&e.to_string()));
                            crate::mainutils::essentials::do_list2env(
                                R_NilValue(),
                                R_NilValue(),
                                call.as_raw(),
                                R_NilValue(),
                            );
                        }
                        "attributes" if elt != R_NilValue() => {
                            crate::sexp::accessors::SET_ATTRIB(env, elt);
                        }
                        "locked"
                            if TYPEOF(elt) == SEXPTYPE::LGLSXP
                                && XLENGTH(elt) > 0
                                && value.try_logical_elt(0).unwrap_or(0) != 0 =>
                        {
                            crate::sexp::envir::lock_environment_raw(env);
                        }
                        _ => {}
                    }
                    owner
                        .require_active()
                        .unwrap_or_else(|e| error(&e.to_string()));
                }
            }
        }
        owner
            .require_active()
            .unwrap_or_else(|e| error(&e.to_string()));
        if let Some(pending) = pending.as_mut() {
            pending.committed = true;
        }
        env
    }
}

fn unserialize_raw(
    icon: SEXP,
    hook_data: SEXP,
    lazy_restore: Option<LazyLoadRestore>,
) -> SEXP {
    {
        if icon.is_null() {
            error("read error");
        }

        let owner = current_owner();
        let _pin = owner.pin().unwrap_or_else(|e| error(&e.to_string()));
        owner
            .require_active()
            .unwrap_or_else(|e| error(&e.to_string()));
        let input = owner
            .sexp(icon)
            .and_then(|value| value.into_owned())
            .unwrap_or_else(|e| error(&e.to_string()));
        let _hook = if hook_data.is_null() {
            None
        } else {
            Some(
                owner
                    .sexp(hook_data)
                    .and_then(|value| value.into_owned())
                    .unwrap_or_else(|e| error(&e.to_string())),
            )
        };
        // Must be RAWSXP
        let stype = TYPEOF(icon);
        if stype != SEXPTYPE::RAWSXP {
            error("not a proper raw vector");
        }

        if ALTREP(input.as_raw()) != 0 {
            input
                .materialize_compact_payload_for_raw()
                .unwrap_or_else(|e| error(&e.to_string()));
        }
        owner
            .require_active()
            .unwrap_or_else(|e| error(&e.to_string()));
        let header = input.header();
        let len = usize::try_from(input.len()).unwrap_or_else(|_| error("read error"));
        if len == 0 {
            error("read error");
        }
        let lease = header
            .payload_lease()
            .unwrap_or_else(|| error("raw input has no initialized payload"));
        // Copy canonical cells before any restoration callback. The reader
        // cannot alias an R payload that reentrant code may resize or mutate.
        let reserve = |bytes| {
            owner
                .with_arena(|arena| arena.reserve_workspace(bytes))
                .map_err(|error| std::io::Error::other(error.to_string()))?
                .ok_or_else(super::owned_input::memory_error)
        };
        let reservation = reserve(len).unwrap_or_else(|failure| error(&failure.to_string()));
        let mut data = super::owned_input::InputBytes::new(reservation, len)
            .unwrap_or_else(|failure| error(&failure.to_string()));
        for index in 0..len {
            std::io::Write::write_all(
                &mut data,
                &[lease
                    .byte_elt(index)
                    .unwrap_or_else(|| error("invalid raw input payload"))],
            )
            .unwrap_or_else(|failure| error(&failure.to_string()));
        }
        unserialize_owned_input(&owner, &data, hook_data, lazy_restore)
    }
}

/// Decode an already admitted immutable native snapshot. Callers retain the
/// original hook graph across restoration callbacks; no borrowed R payload or
/// interpreter field loan may span this operation.
fn unserialize_owned_input(
    owner: &crate::sexp::owner::OwnerToken<'_>,
    data: &[u8],
    hook_data: SEXP,
    lazy_restore: Option<LazyLoadRestore>,
) -> SEXP {
    {
        let _pin = owner.pin().unwrap_or_else(|e| error(&e.to_string()));
        owner
            .require_active()
            .unwrap_or_else(|e| error(&e.to_string()));
        if data.is_empty() {
            error("read error");
        }
        let reserve = |bytes| {
            owner
                .with_arena(|arena| arena.reserve_workspace(bytes))
                .map_err(|error| std::io::Error::other(error.to_string()))?
                .ok_or_else(super::owned_input::memory_error)
        };
        let decompressed = super::owned_input::expand(data, reserve).unwrap_or_else(|failure| {
            if failure.kind() == std::io::ErrorKind::OutOfMemory {
                error(&failure.to_string());
            }
            error("read error")
        });
        let payload = decompressed.as_deref().unwrap_or(data);
        let mut reader = BinaryReader::new(payload);
        reader.lazy_restore = lazy_restore;
        reader.set_persist_hook(hook_data);
        decode_stream_object(owner, &mut reader)
    }
}

/// Decode a caller-owned immutable native snapshot without copying it into R.
/// The caller keeps its admitted backing storage alive and unchanged through
/// restoration callbacks. A checked hook and operation pin retain the original
/// runtime; no R payload view or interpreter field loan crosses a callback.
pub(crate) fn unserialize_native_snapshot(
    owner: &crate::sexp::owner::OwnerToken<'_>,
    data: &[u8],
    hook: &crate::sexp::object::Sexp<'_>,
) -> crate::sexp::object::SexpResult<crate::sexp::object::Sexp<'static>> {
    let _pin = owner.pin()?;
    owner.require_active()?;
    let hook = owner.sexp(hook.as_raw())?.into_owned()?;
    let lazy_restore =
        (hook.typeof_() == SEXPTYPE::ENVSXP).then(|| LazyLoadRestore { data: hook.clone() });
    let hook_data = if lazy_restore.is_some() {
        R_NilValue()
    } else {
        hook.as_raw()
    };
    // SAFETY: the native slice is immutable and its caller owns its complete
    // backing across callbacks. The original hook graph is owned above.
    let result = { unserialize_owned_input(owner, data, hook_data, lazy_restore) };
    owner.require_active()?;
    owner.sexp(result)?.into_owned()
}

/// Expand a complete workspace before parsing its magic and serialized body.
pub(crate) fn expand_workspace_snapshot(
    owner: &crate::sexp::owner::OwnerToken<'_>,
    bytes: &[u8],
) -> std::io::Result<Option<super::owned_input::InputBytes>> {
    super::owned_input::expand_workspace(bytes, |size| {
        owner
            .with_arena(|arena| arena.reserve_workspace(size))
            .map_err(|error| std::io::Error::other(error.to_string()))?
            .ok_or_else(super::owned_input::memory_error)
    })
}

pub(crate) fn decode_workspace_snapshot(
    owner: &crate::sexp::owner::OwnerToken<'_>,
    body: &[u8],
    ascii: bool,
) -> crate::sexp::object::SexpResult<crate::sexp::object::Sexp<'static>> {
    let mut reader = BinaryReader::new(body);
    // SAFETY: the caller keeps the complete, admitted body alive through
    // decoding; no R payload loan is live.
    let decoded = { decode_stream_object(owner, &mut reader) };
    owner.require_active()?;
    let trailing = &body[reader.pos..];
    if !(trailing.is_empty() || ascii && trailing.iter().all(u8::is_ascii_whitespace)) {
        crate::sexp::context::r_error("unexpected data after workspace");
    }
    owner.sexp(decoded)?.into_owned()
}

/// Parse the stream header (format, versions, native encoding) and decode the
/// single object that follows. The reader's byte source — slice or pull —
/// determines how many input bytes this consumes.
fn decode_stream_object(
    owner: &crate::sexp::owner::OwnerToken<'_>,
    reader: &mut BinaryReader<'_>,
) -> SEXP {
    {
        // Read format header: two bytes (`A\n`, `B\n`, or `X\n`).
        let mut fmt1 = reader.read_byte().unwrap_or(0);
        let mut fmt2 = reader.read_byte().unwrap_or(0);
        // GNU InFormat accepts exactly the newline left by an ASCII string,
        // followed by the next ASCII header. Other leading whitespace and
        // newlines before binary/XDR headers remain invalid.
        if fmt1 == b'\n' && fmt2 == b'A' {
            fmt1 = b'A';
            fmt2 = reader.read_byte().unwrap_or(0);
        }
        if (fmt1 != b'A' && fmt1 != b'B' && fmt1 != b'X') || fmt2 != b'\n' {
            error("unknown input format");
        }
        reader.set_ascii_body(fmt1 == b'A');
        reader.set_xdr_body(fmt1 == b'X');

        // Read version
        let version = reader.read_i32().unwrap_or(0);
        if version != 2 && version != 3 {
            error("version not supported");
        }

        // Read writer_version and min_reader_version
        let _writer_version = reader.read_i32().unwrap_or(0);
        let _min_reader_version = reader.read_i32().unwrap_or(0);

        // Read native encoding length (version 3)
        if version == 3 {
            let nelen = reader.read_i32().unwrap_or(0);
            if nelen > 0 {
                // Skip encoding bytes
                let _ = reader.read_bytes(nelen as usize);
            }
        }

        // No partially decoded graph exists yet. At pressure, collect only
        // after actual growth since the previous collection. A large live
        // namespace otherwise triggers a full trace before every tiny lazy
        // record, even when the preceding trace reclaimed nothing. Admission
        // caps still apply to every allocation; input and restoration hooks
        // remain owning roots across collection callbacks.
        let collect = owner
            .with_arena(|arena| {
                arena.budget_pressure_warrants_full_gc() && arena.growth_warrants_gc()
            })
            .unwrap_or_else(|e| error(&e.to_string()));
        if collect {
            owner.full_gc().unwrap_or_else(|e| error(&e.to_string()));
            owner
                .require_active()
                .unwrap_or_else(|e| error(&e.to_string()));
        }

        // Read the object
        let mut ref_table = ReadRefTable::for_owner(&owner);
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ReadItemInternal(reader, &mut ref_table)
        }));
        owner
            .require_active()
            .unwrap_or_else(|e| error(&e.to_string()));
        match outcome {
            Ok(Ok(s)) => owner
                .sexp(s)
                .unwrap_or_else(|e| error(&e.to_string()))
                .as_raw(),
            Ok(Err(message)) => error(&message),
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }
}

/// `unserialize(connection)`: decode exactly one serialized object from the
/// connection, pulling bytes on demand (GNU `R_InitConnInPStream` /
/// `ConnReadChar` data flow). Trailing bytes stay unread in the connection.
fn R_unserialize_from_conn(index: std::os::raw::c_int, hook: SEXP) -> SEXP {
    {
        let owner = current_owner();
        let _pin = owner.pin().unwrap_or_else(|e| error(&e.to_string()));
        owner
            .require_active()
            .unwrap_or_else(|e| error(&e.to_string()));

        let hook = owner
            .sexp(hook)
            .and_then(|s| s.into_owned())
            .unwrap_or_else(|e| error(&e.to_string()));
        let token = crate::mainutils::connections::connection_token(index)
            .unwrap_or_else(|message| error(&message));
        let mut fill = |buf: &mut super::owned_input::InputBytes, need: usize| -> bool {
            // Validate even if the reader already has the requested bytes.
            // A restoration hook can close/reuse a slot between reads.
            owner
                .require_active()
                .unwrap_or_else(|e| error(&e.to_string()));
            let index = token
                .require_current()
                .unwrap_or_else(|message| error(&message));
            while buf.len() < need {
                if !buf
                    .push_from(|| {
                        let byte = crate::mainutils::connections::connection_fgetc(index);
                        (byte >= 0).then_some(byte as u8)
                    })
                    .unwrap_or_else(|e| error(&e.to_string()))
                {
                    return false;
                }
            }
            true
        };
        // Compression belongs to the connection (gzfile/gzcon), as in GNU.
        // Sniffing and buffering a gzip payload here consumed later objects
        // and made hook-observed cursor positions describe the wrong stream.
        let mut reader = BinaryReader::new_pull(&owner, &mut fill);
        configure_conn_hooks(&owner, &mut reader, hook.as_raw());
        let result = decode_stream_object(&owner, &mut reader);
        let result = owner.sexp(result).unwrap_or_else(|e| error(&e.to_string()));
        let leftover = reader.leftover();
        if !leftover.is_empty() {
            owner
                .require_active()
                .unwrap_or_else(|e| error(&e.to_string()));
            let index = token
                .require_current()
                .unwrap_or_else(|message| error(&message));
            crate::mainutils::connections::connection_pushback(index, leftover);
        }
        result.as_raw()
    }
}

/// Apply the `refhook` argument of `unserialize(connection, refhook)`.
fn configure_conn_hooks(
    owner: &crate::sexp::owner::OwnerToken<'_>,
    reader: &mut BinaryReader<'_>,
    hook: SEXP,
) {
    {
        if !hook.is_null() && hook != R_NilValue() && TYPEOF(hook) == SEXPTYPE::ENVSXP {
            let data = owner
                .sexp(hook)
                .and_then(|s| s.into_owned())
                .unwrap_or_else(|e| error(&e.to_string()));
            reader.lazy_restore = Some(LazyLoadRestore { data });
        } else {
            reader.set_persist_hook(hook);
        }
    }
}

// ---------------------------------------------------------------------------
// R_serializeb
// ---------------------------------------------------------------------------

pub fn R_serializeb(object: SEXP, icon: SEXP, xdr: SEXP, Sversion: SEXP, fun: SEXP) -> SEXP {
    R_serialize_with_xdr(object, icon, R_NilValue(), xdr, Sversion, fun)
}

// ---------------------------------------------------------------------------
// do_serialize (dispatch for serialize/unserialize builtins)
// ---------------------------------------------------------------------------

/// GNU base's wrapper supplies `(object, connection, type, version, refhook)`.
/// Keep this adapter separate from the public six-argument builtin contract.
pub fn do_serialize_internal(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    {
        let object = require_arg(args, 0);
        let conn = require_arg(args, 1);
        let format = asInteger(require_arg(args, 2));
        let version = require_arg(args, 3);
        let hook = require_arg(args, 4);
        let (ascii, xdr) = match format {
            0 => (0, 1),
            1 => (1, 0),
            2 => (crate::sexp::ffi::NA_LOGICAL, 0),
            3 => (0, 0),
            _ => error("invalid serialization format"),
        };
        let ascii = Rf_ScalarLogical(ascii);
        let xdr = Rf_ScalarLogical(xdr);
        let raw = R_serialize_with_xdr(object, conn, ascii, xdr, version, hook);
        if TYPEOF(conn) == SEXPTYPE::INTSXP {
            let bytes = raw_to_bytes(raw);
            crate::mainutils::connections::connection_write_bytes(asInteger(conn), &bytes);
            return R_NilValue();
        }
        raw
    }
}

pub fn do_serialize(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    {
        let _ = (call, op, env);
        if args.is_null() || args == R_NilValue() {
            error("argument \"connection\" is missing, with no default");
        }

        // serialize(object, connection, ascii, xdr, version, refhook)
        let object = arg_by_name_or_position(args, "object", 0);
        let conn = arg_by_name_or_position(args, "connection", 1);
        let has_object = arg_present_by_name_or_position(args, "object", 0);
        if !has_object || object.is_null() || object == R_MissingArg() {
            error("argument \"object\" is missing, with no default");
        }
        let has_conn = arg_present_by_name_or_position(args, "connection", 1);
        if !has_conn || conn.is_null() || conn == R_MissingArg() {
            error("argument \"connection\" is missing, with no default");
        }
        let mut ascii = arg_by_name_or_position(args, "ascii", 2);
        let mut version = arg_by_name_or_position(args, "version", 4);
        if version == R_NilValue()
            && !ascii.is_null()
            && ascii != R_NilValue()
            && TYPEOF(ascii) != SEXPTYPE::LGLSXP
            && scalar_integer_value(ascii).is_some()
            && optional_arg(args, 3) == R_NilValue()
        {
            version = ascii;
            ascii = R_NilValue();
        }

        let xdr = arg_by_name_or_position(args, "xdr", 3);
        let hook = arg_by_name_or_position(args, "refhook", 5);
        let raw = R_serialize_with_xdr(object, R_NilValue(), ascii, xdr, version, hook);
        if TYPEOF(conn) == SEXPTYPE::INTSXP {
            let bytes = raw_to_bytes(raw);
            crate::mainutils::connections::connection_write_bytes(INTEGER_ELT(conn, 0), &bytes);
            return R_NilValue();
        }
        raw
    }
}

// ---------------------------------------------------------------------------
// do_serializeToConn
// ---------------------------------------------------------------------------

pub fn do_serializeToConn(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    {
        let _ = (call, op, env);
        if args.is_null() || args == R_NilValue() {
            error("wrong number of arguments");
        }

        let object = CAR(args);
        let conn = CADR(args);
        let raw = R_serialize_with_xdr(
            object,
            R_NilValue(),
            R_NilValue(),
            R_NilValue(),
            R_NilValue(),
            R_NilValue(),
        );
        let index = usize::try_from(INTEGER_ELT(conn, 0)).unwrap_or(0);
        let path = crate::mainutils::connections::connection_description(index);
        let bytes = raw_to_bytes(raw);
        if std::fs::write(&path, &bytes).is_err() {
            error("cannot write to connection");
        }
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_unserializeFromConn
// ---------------------------------------------------------------------------

pub fn do_unserializeFromConn(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    {
        let _ = (call, op, env);
        if args.is_null() || args == R_NilValue() {
            error("wrong number of arguments");
        }

        let owner = current_owner();
        let _pin = owner.pin().unwrap_or_else(|e| error(&e.to_string()));
        let _arguments = owner.sexp(args).unwrap_or_else(|e| error(&e.to_string()));
        // A connection/class provider can detach the argument tail and collect.
        // Retain both actual operands before invoking either provider.
        let conn = owner
            .sexp(CAR(args))
            .unwrap_or_else(|e| error(&e.to_string()));
        let hook = owner
            .sexp(arg_by_name_or_position(args, "refhook", 1))
            .unwrap_or_else(|e| error(&e.to_string()));
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let result = if conn.typeof_() == SEXPTYPE::RAWSXP {
                R_unserialize(conn.as_raw(), hook.as_raw())
            } else if conn.typeof_() == SEXPTYPE::INTSXP
                && conn.len() == 1
                && crate::mainutils::objects::inherits2(conn.as_raw(), c"connection") != 0
            {
                let index = conn
                    .try_integer_elt(0)
                    .unwrap_or_else(|e| error(&e.to_string()));
                owner
                    .require_active()
                    .unwrap_or_else(|e| error(&e.to_string()));
                R_unserialize_from_conn(index, hook.as_raw())
            } else {
                error("'connection' must be a connection");
            };
            owner
                .require_active()
                .unwrap_or_else(|e| error(&e.to_string()));
            owner.sexp(result).unwrap_or_else(|e| error(&e.to_string()))
        }));
        owner
            .require_active()
            .unwrap_or_else(|e| error(&e.to_string()));
        match outcome {
            Ok(value) => value.as_raw(),
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }
}

// ---------------------------------------------------------------------------
// Lazy-load database functions
// ---------------------------------------------------------------------------

pub fn do_lazyLoadDBflush(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    {
        let _ = (call, op, env);
        let file = require_arg(args, 0);
        let _ = sexp_to_path(file);
        R_NilValue()
    }
}

/// The failed codec has dropped every temporary reservation and constructs no
/// R graph. At actual resource denial only, the caller's owned operands and
/// native snapshot permit one full collection and retry in the same runtime.
pub(crate) fn decode_lazy_record(
    owner: &crate::sexp::owner::OwnerToken<'_>,
    encoded: &[u8],
    mode: u8,
) -> Result<super::owned_input::InputBytes, super::raw_codecs::DecodeError> {
    match super::raw_codecs::decode(owner, encoded, mode) {
        Err(super::raw_codecs::DecodeError::Resource(_)) => {
            owner
                .full_gc()
                .map_err(|failure| super::raw_codecs::DecodeError::Resource(failure.to_string()))?;
            owner
                .require_active()
                .map_err(|failure| super::raw_codecs::DecodeError::Resource(failure.to_string()))?;
            super::raw_codecs::decode(owner, encoded, mode)
        }
        result => result,
    }
}

pub fn do_lazyLoadDBfetch(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    {
        let _ = (call, op, env);
        let key = require_arg(args, 0);
        let file = require_arg(args, 1);
        let compsxp = require_arg(args, 2);
        let hook = require_arg(args, 3);
        // Portable pinned data uses the same original lazy-load stream, while
        // retaining all selected operands through provider and reader callbacks.
        // Inspection of ordinary file vectors invokes no provider accessor.
        let owner = current_owner();
        let _pin = owner.pin().unwrap_or_else(|e| error(&e.to_string()));
        // Retain the entire native call, environment and argument graph before
        // making the codec retry collecting. NULL compatibility arguments
        // have no R graph to retain. No field or payload loan crosses GC.
        let _entry_roots = [call, op, args, env].map(|value| {
            (!value.is_null()).then(|| {
                owner
                    .sexp(value)
                    .and_then(|value| value.into_owned())
                    .unwrap_or_else(|failure| error(&failure.to_string()))
            })
        });
        // Retain selected operands before coercion/provider callbacks too.
        let operands = [key, compsxp, hook].map(|value| {
            owner
                .sexp(value)
                .and_then(|value| value.into_owned())
                .unwrap_or_else(|e| error(&e.to_string()))
        });
        let file_owned = owner
            .sexp(file)
            .and_then(crate::sexp::object::Sexp::into_owned)
            .unwrap_or_else(|e| error(&e.to_string()));
        if let Some(image) = crate::library::portable_package::database(&file_owned)
            .unwrap_or_else(|failure| error(&failure.to_string()))
        {
            let own = |value| {
                owner
                    .sexp(value)
                    .and_then(crate::sexp::object::Sexp::into_owned)
                    .unwrap_or_else(|failure| error(&failure.to_string()))
            };
            return image
                .fetch(own(key), file_owned, own(compsxp), own(hook))
                .unwrap_or_else(|message| error(&message))
                .as_raw();
        }
        if crate::library::datasets::is_database(&file_owned)
            .unwrap_or_else(|e| error(&e.to_string()))
        {
            let own = |value| {
                owner
                    .sexp(value)
                    .and_then(crate::sexp::object::Sexp::into_owned)
                    .unwrap_or_else(|e| error(&e.to_string()))
            };
            return crate::library::datasets::fetch(own(key), file_owned, own(compsxp), own(hook))
                .unwrap_or_else(|message| error(&message))
                .as_raw();
        }
        let compressed = asInteger(compsxp);
        owner
            .require_active()
            .unwrap_or_else(|e| error(&e.to_string()));
        let mode = match compressed {
            0 => 0,
            2 => 2,
            3 => 3,
            _ => 1,
        };
        let raw = readRawFromFile(file, key);
        let raw_guard = protect(raw);
        let encoded = super::compress::raw_snapshot(&owner, raw, mode, false);
        drop(raw_guard);
        let data = if mode == 0 {
            encoded
        } else {
            let decoded = match decode_lazy_record(&owner, &encoded, mode) {
                Ok(decoded) => decoded,
                Err(super::raw_codecs::DecodeError::Resource(message)) => error(&message),
                Err(super::raw_codecs::DecodeError::Invalid) => {
                    let file_name = sexp_to_path(file);
                    error(&format!(
                        "lazy-load database '{}' is corrupt",
                        file_name.display()
                    ));
                }
            };
            drop(encoded);
            decoded
        };
        // The native bytes remain admitted across every nested restore hook.
        // Avoid the former native -> R raw -> native round trip entirely.
        let value = unserialize_native_snapshot(&owner, &data, &operands[2])
            .unwrap_or_else(|failure| error(&failure.to_string()));
        let mut val = value.as_raw();
        let mut val_guard = protect(val);
        drop(data);
        if TYPEOF(val) == SEXPTYPE::PROMSXP {
            val = Rf_eval(val, R_GlobalEnv());
            val_guard = protect(val);
            if !val.is_null() {
                SET_NAMED(val, 2);
            }
        }
        let _ = val_guard;
        val
    }
}

pub fn do_getVarsFromFrame(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    {
        let _ = (call, op, env);
        let vars = require_arg(args, 0);
        let env = require_arg(args, 1);
        let forcesxp = require_arg(args, 2);
        R_getVarsFromFrame(vars, env, forcesxp)
    }
}

pub fn do_lazyLoadDBinsertValue(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    {
        let _ = (call, op, env);
        let value = require_arg(args, 0);
        let file = require_arg(args, 1);
        let ascii = require_arg(args, 2);
        let compsxp = require_arg(args, 3);
        let hook = require_arg(args, 4);
        R_lazyLoadDBinsertValue(value, file, ascii, compsxp, hook)
    }
}

#[cfg(test)]
#[path = "persistence_tests.rs"]
mod persistence_tests;

#[cfg(test)]
#[path = "connection_stream_tests.rs"]
mod connection_stream_tests;
