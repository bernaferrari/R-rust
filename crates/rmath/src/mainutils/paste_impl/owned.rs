#![forbid(unsafe_code)]
//! Owning paste traversal; native encoding/coercion stays at the explicit bridge.
use crate::sexp::{
    ffi::SEXPTYPE,
    memory::TransientReservation,
    object::{Sexp, SexpError, SexpMut, SexpResult},
    owner::RuntimeAccess,
};
pub(super) type Value = Sexp<'static>;
#[derive(Clone, Copy)]
pub(super) enum Mode {
    Native,
    Utf8,
    Bytes,
}
pub(super) struct Bytes {
    pub(super) data: Vec<u8>,
    pub(super) _reservation: TransientReservation,
}
pub(super) trait Native {
    fn initialize(&self, op: &Value, args: &Value) -> SexpResult<bool>;
    fn boolean(&self, value: &Value, call: &Value) -> SexpResult<bool>;
    fn coerce(&self, value: &Value, env: &Value) -> SexpResult<Value>;
    fn bytes(&self, value: &Value, mode: Mode, parent: &Value) -> SexpResult<Bytes>;
    fn flags(&self, value: &Value) -> (bool, bool);
    fn name(&self, op: &Value) -> String;
}
fn error(message: impl Into<String>) -> SexpError {
    SexpError::EvaluationFailed {
        message: message.into(),
    }
}
fn allocation(object: &'static str) -> SexpError {
    SexpError::AllocationFailed { object }
}
fn cell(value: &Value) -> SexpResult<(Value, Value)> {
    Ok((
        value.try_car()?.into_owned()?,
        value.try_cdr()?.into_owned()?,
    ))
}
fn length(access: &RuntimeAccess, value: &Value) -> SexpResult<i64> {
    let n = value.len();
    access.require_active()?;
    Ok(n)
}
fn element(access: &RuntimeAccess, input: &Value, index: i64) -> SexpResult<Value> {
    let v = input.try_string_elt(index)?.into_owned()?;
    access.require_active()?;
    Ok(v)
}
fn separator(access: &RuntimeAccess, input: &Value, message: &'static str) -> SexpResult<Value> {
    if input.typeof_() != SEXPTYPE::STRSXP || length(access, input)? == 0 {
        return Err(error(message));
    }
    let selected = element(access, input, 0)?;
    if selected.is_na_string() {
        return Err(error(message));
    }
    Ok(selected)
}
fn vector(access: &RuntimeAccess, n: i64) -> SexpResult<SexpMut<'static>> {
    let d = access.domain();
    SexpMut::try_from_checked(
        access
            .allocator(&d)?
            .allocate(|a| Some(a.alloc_vector(SEXPTYPE::STRSXP, n)))?,
    )
}
fn character(access: &RuntimeAccess, bytes: &[u8]) -> SexpResult<Value> {
    // The pre-existing mkCharCE bridge ignored the requested encoding. Keep
    // those bytes/constructor semantics; encoding-flag parity is separate.
    let d = access.domain();
    access
        .allocator(&d)?
        .allocate(|a| Some(a.alloc_charsxp(bytes)))
}
fn zero(access: &RuntimeAccess, collapse: bool) -> SexpResult<Value> {
    let mut result = vector(access, i64::from(collapse))?;
    if collapse {
        result.try_set_string_elt(0, character(access, b"")?)?;
    }
    access.require_active()?;
    Ok(result.freeze())
}
fn join(parent: &Value, parts: &[Bytes], separator: &Bytes) -> SexpResult<Bytes> {
    let count = parts.len().saturating_sub(1);
    let size = parts
        .iter()
        .try_fold(0usize, |n, p| n.checked_add(p.data.len()))
        .and_then(|n| {
            separator
                .data
                .len()
                .checked_mul(count)
                .and_then(|s| n.checked_add(s))
        })
        .ok_or_else(|| error("result would exceed 2^31-1 bytes"))?;
    if size > i32::MAX as usize {
        return Err(error("result would exceed 2^31-1 bytes"));
    }
    let node = parent.allocation()?;
    let reservation = node
        .heap_identity()
        .reserve_payload_bytes(node, size)
        .ok_or_else(|| allocation("paste workspace budget"))?;
    let mut data = Vec::new();
    data.try_reserve_exact(size)
        .map_err(|_| allocation("paste concatenation"))?;
    for (i, p) in parts.iter().enumerate() {
        if i != 0 {
            data.extend_from_slice(&separator.data);
        }
        data.extend_from_slice(&p.data);
    }
    Ok(Bytes {
        data,
        _reservation: reservation,
    })
}
fn empty(parent: &Value) -> SexpResult<Bytes> {
    let node = parent.allocation()?;
    Ok(Bytes {
        data: Vec::new(),
        _reservation: node
            .heap_identity()
            .reserve_payload_bytes(node, 0)
            .ok_or_else(|| allocation("paste workspace"))?,
    })
}
fn select(access: &RuntimeAccess, x: &Value, j: i64, i: i64) -> SexpResult<Option<Value>> {
    let value = x.try_vector_elt(j)?.into_owned()?;
    access.require_active()?;
    let n = length(access, &value)?;
    if n == 0 {
        Ok(None)
    } else {
        Ok(Some(element(access, &value, i % n)?))
    }
}
pub(super) fn evaluate(
    access: &RuntimeAccess,
    native: &impl Native,
    call: Value,
    op: Value,
    args: Value,
    env: Value,
) -> SexpResult<Value> {
    let use_sep = native.initialize(&op, &args)?;
    access.require_active()?;
    let argument = |index: usize| -> SexpResult<Value> {
        let mut rest = args.clone();
        for _ in 0..index {
            rest = rest.try_cdr()?.into_owned()?;
        }
        if rest.is_nil() {
            Ok(access.domain().nil())
        } else {
            Ok(cell(&rest)?.0)
        }
    };
    let x = argument(0)?;
    if !matches!(x.typeof_(), SEXPTYPE::VECSXP | SEXPTYPE::EXPRSXP) {
        return Err(error("invalid first argument"));
    }
    let nx = length(access, &x)?;
    // As in GNU, validate then select sep. Its provider may change subsequent
    // argument cells, so capture collapse/recycle only after these callbacks.
    let sep = if use_sep {
        let input = argument(1)?;
        separator(access, &input, "invalid separator")?;
        Some(element(access, &input, 0)?)
    } else {
        None
    };
    let collapse = argument(if use_sep { 2 } else { 1 })?;
    let recycle = argument(if use_sep { 3 } else { 2 })?;
    let recycle = native.boolean(&recycle, &call)?;
    access.require_active()?;
    let do_collapse = !collapse.is_nil();
    if do_collapse {
        separator(access, &collapse, "invalid 'collapse' argument")?;
    }
    if nx == 0 {
        return zero(access, do_collapse);
    }
    // GNU visits/coerces each current element in order. A previous callback may
    // change a later element, so this is not an eager snapshot of the list.
    let mut maxlen = 0;
    for j in 0..nx {
        let input = x.try_vector_elt(j)?.into_owned()?;
        access.require_active()?;
        let input = if input.typeof_() != SEXPTYPE::STRSXP {
            let value = native.coerce(&input, &env)?;
            access.require_active()?;
            if value.typeof_() != SEXPTYPE::STRSXP {
                return Err(error(format!(
                    "non-string argument to .Internal({})",
                    native.name(&op)
                )));
            }
            SexpMut::try_from_checked(x.clone())?.try_set_vector_elt(j, value.clone())?;
            value
        } else {
            input
        };
        let n = length(access, &input)?;
        if recycle && n == 0 {
            return zero(access, do_collapse);
        }
        maxlen = maxlen.max(n);
    }
    if maxlen == 0 {
        return zero(access, do_collapse);
    }
    let mut result = vector(access, maxlen)?;
    for i in 0..maxlen {
        let (mut utf8, mut bytes) = if nx > 1 {
            sep.as_ref().map_or((false, false), |s| native.flags(s))
        } else {
            (false, false)
        };
        // Keep GNU's flag/width/copy passes and callback order. Each selected
        // character has an actual owner through translation; no borrowed span
        // crosses a provider or output-allocation callback.
        for j in 0..nx {
            if let Some(v) = select(access, &x, j, i)? {
                let f = native.flags(&v);
                utf8 |= f.0;
                bytes |= f.1;
            }
        }
        let mode = if bytes {
            Mode::Bytes
        } else if utf8 {
            Mode::Utf8
        } else {
            Mode::Native
        };
        let mut width = 0usize;
        for j in 0..nx {
            if let Some(v) = select(access, &x, j, i)? {
                width = width
                    .checked_add(native.bytes(&v, mode, &args)?.data.len())
                    .ok_or_else(|| error("result would exceed 2^31-1 bytes"))?;
            }
        }
        let separator = if let Some(s) = &sep {
            native.bytes(s, mode, &args)?
        } else {
            empty(&args)?
        };
        let gaps = usize::try_from(nx - 1).map_err(|_| allocation("paste input count"))?;
        width = separator
            .data
            .len()
            .checked_mul(gaps)
            .and_then(|s| width.checked_add(s))
            .ok_or_else(|| error("result would exceed 2^31-1 bytes"))?;
        if width > i32::MAX as usize {
            return Err(error("result would exceed 2^31-1 bytes"));
        }
        let mut parts = Vec::new();
        parts
            .try_reserve_exact(usize::try_from(nx).map_err(|_| allocation("paste input count"))?)
            .map_err(|_| allocation("paste row"))?;
        for j in 0..nx {
            parts.push(if let Some(v) = select(access, &x, j, i)? {
                native.bytes(&v, mode, &args)?
            } else {
                empty(&args)?
            });
        }
        let joined = join(&args, &parts, &separator)?;
        let text = character(access, &joined.data)?;
        result.try_set_string_elt(i, text)?;
        access.require_active()?;
    }
    if do_collapse {
        let collapse = element(access, &collapse, 0)?;
        let n = result.freeze();
        let (mut utf8, mut bytes) = native.flags(&collapse);
        for i in 0..maxlen {
            let v = element(access, &n, i)?;
            let f = native.flags(&v);
            utf8 |= f.0;
            bytes |= f.1;
        }
        let mode = if bytes {
            Mode::Bytes
        } else if utf8 {
            Mode::Utf8
        } else {
            Mode::Native
        };
        let separator = native.bytes(&collapse, mode, &args)?;
        // Width pass before copy, as in GNU; selections remain independently owned.
        for i in 0..maxlen {
            let v = element(access, &n, i)?;
            native.bytes(&v, mode, &args)?;
        }
        let mut parts = Vec::new();
        parts
            .try_reserve_exact(
                usize::try_from(maxlen).map_err(|_| allocation("paste collapse length"))?,
            )
            .map_err(|_| allocation("paste collapse"))?;
        for i in 0..maxlen {
            let v = element(access, &n, i)?;
            parts.push(native.bytes(&v, mode, &args)?);
        }
        let joined = join(&args, &parts, &separator)?;
        let mut output = vector(access, 1)?;
        output.try_set_string_elt(0, character(access, &joined.data)?)?;
        access.require_active()?;
        return Ok(output.freeze());
    }
    access.require_active()?;
    Ok(result.freeze())
}
