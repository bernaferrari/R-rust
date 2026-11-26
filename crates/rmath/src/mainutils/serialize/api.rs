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
        let _ = (ascii, xdr, Sversion);
        let data = crate::mainutils::rbg1::write_object_hooked(object, fun)
            .unwrap_or_else(|failure| error(&failure));
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
        let _ = ascii;
        let _object_guard = protect(bindings);
        let frame = crate::mainutils::rbg1::write_object_hooked(bindings, R_NilValue())
            .unwrap_or_else(|failure| error(&failure));
        let mut writer = BinaryWriter::admitted(owner);
        writer.write_bytes(prefix);
        writer.write_bytes(&frame);
        owner
            .require_active()
            .unwrap_or_else(|e| error(&e.to_string()));
        writer.into_output()
    }
}

/// Unserialize an R object from a raw vector.
pub fn R_unserialize(icon: SEXP, fun: SEXP) -> SEXP {
    {
        let owner = current_owner();
        let _pin = owner.pin().unwrap_or_else(|e| error(&e.to_string()));
        owner
            .require_active()
            .unwrap_or_else(|e| error(&e.to_string()));
        let bytes = raw_to_bytes(icon);
        let _fun_guard = protect(fun);
        let hook = if !fun.is_null() && TYPEOF(fun) == SEXPTYPE::CLOSXP {
            fun
        } else {
            R_NilValue()
        };
        // Compressed inputs expand transparently, as in GNU readRDS/unserialize.
        let expanded = super::owned_input::expand(&bytes, |size| {
            owner
                .with_arena(|arena| arena.reserve_workspace(size))
                .map_err(|e| std::io::Error::other(e.to_string()))?
                .ok_or_else(super::owned_input::memory_error)
        })
        .unwrap_or_else(|_| error("unserialize exceeds session memory limit"));
        let plain: &[u8] = expanded.as_deref().unwrap_or(&bytes);
        let value = crate::mainutils::rbg1::read_object_hooked(plain, hook)
            .unwrap_or_else(|failure| error(&failure));
        owner
            .require_active()
            .unwrap_or_else(|e| error(&e.to_string()));
        value
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
    let hook_data = hook.as_raw();
    // RBG1 frames materialize through the graph reader; anything else is a
    // legacy GNU stream during the migration.
    if data.len() >= 4 && data[..4] == *crate::mainutils::rbg1::MAGIC {
        let frame_len = crate::mainutils::rbg1::frame_total_len(data).unwrap_or(0);
        if frame_len != data.len() {
            return Err(crate::sexp::object::SexpError::EvaluationFailed {
                message: "trailing bytes after RBG1 lazy record".to_string(),
            });
        }
        let value = crate::mainutils::rbg1::read_object_hooked(data, hook_data)
            .map_err(|message| crate::sexp::object::SexpError::EvaluationFailed { message })?;
        owner.require_active()?;
        return owner.sexp(value)?.into_owned();
    }
    Err(crate::sexp::object::SexpError::EvaluationFailed {
        message: "not an RBG1 serialized object".to_string(),
    })
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
    if body.len() >= 4 && body[..4] == *crate::mainutils::rbg1::MAGIC {
        owner.require_active()?;
        let decoded = crate::mainutils::rbg1::read_object_hooked(body, R_NilValue())
            .map_err(|message| crate::sexp::object::SexpError::EvaluationFailed { message })?;
        let total = crate::mainutils::rbg1::frame_total_len(body).unwrap_or(body.len());
        let trailing = &body[total.min(body.len())..];
        if !(trailing.is_empty() || ascii && trailing.iter().all(u8::is_ascii_whitespace)) {
            crate::sexp::context::r_error("unexpected data after workspace");
        }
        return owner.sexp(decoded)?.into_owned();
    }
    Err(crate::sexp::object::SexpError::EvaluationFailed {
        message: "not an RBG1 workspace body".to_string(),
    })
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
        let reservation = owner
            .with_arena(|a| a.reserve_workspace(0))
            .unwrap_or_else(|e| error(&e.to_string()))
            .unwrap_or_else(|| error("unserialize exceeds session memory limit"));
        let mut buf = super::owned_input::InputBytes::new(reservation, 0)
            .unwrap_or_else(|_| error("unserialize exceeds session memory limit"));
        let fill = |buf: &mut super::owned_input::InputBytes, need: usize| -> bool {
            // Validate even if the buffer already holds the requested bytes.
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
        // RBG1 frames are self-delimiting: probe bytes stay in this buffer
        // through the same accounting, so nothing is ever lost — even when
        // decoding fails mid-stream.
        let header_len = crate::mainutils::rbg1::HEADER_LEN;
        let total = {
            if !fill(&mut buf, header_len) {
                error("truncated RBG1 stream header");
            }
            if buf.len() < 4
                || buf[0] != b'R'
                || buf[1] != b'B'
                || buf[2] != b'G'
                || buf[3] != b'1'
            {
                error("not an RBG1 serialization stream");
            }
            let compressed_len =
                u64::from_le_bytes(buf[17..25].try_into().unwrap()) as usize;
            let total = header_len + compressed_len;
            if !fill(&mut buf, total) {
                error("truncated RBG1 stream frame");
            }
            total
        };
        let frame = buf[..total].to_vec();
        owner
            .require_active()
            .unwrap_or_else(|e| error(&e.to_string()));
        let value = crate::mainutils::rbg1::read_object_hooked(&frame, hook.as_raw())
            .unwrap_or_else(|failure| error(&failure));
        let result = owner.sexp(value).unwrap_or_else(|e| error(&e.to_string()));
        let leftover = &buf[total..];
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
}// ---------------------------------------------------------------------------
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

