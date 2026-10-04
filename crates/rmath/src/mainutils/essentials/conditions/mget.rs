//! Original-domain mget operands and sequential lazy-binding evaluation.
use crate::sexp::{
    ffi::{NA_INTEGER, R_xlen_t, SEXP, SEXPTYPE},
    object::{Sexp, SexpError, SexpMut, SexpResult},
    owner::{OwnerToken, RuntimeAccess, with_runtime},
};

fn failure(message: impl Into<String>) -> SexpError {
    SexpError::EvaluationFailed {
        message: message.into(),
    }
}

struct Argument {
    value: Sexp<'static>,
    tag: Sexp<'static>,
}
struct Arguments(Vec<Argument>);
impl Arguments {
    fn capture(mut cursor: Sexp<'static>) -> SexpResult<Self> {
        let mut arguments = Vec::new();
        let mut seen = std::collections::HashSet::new();
        while !cursor.is_nil() {
            let identity = cursor
                .allocation()?
                .link()
                .ok_or(SexpError::StaleAllocation)?;
            seen.try_reserve(1)
                .map_err(|_| failure("mget argument allocation failed"))?;
            if !seen.insert(identity) {
                return Err(failure("cyclic mget argument list"));
            }
            arguments
                .try_reserve(1)
                .map_err(|_| failure("mget argument allocation failed"))?;
            arguments.push(Argument {
                value: cursor.try_car()?.into_owned()?,
                tag: cursor.try_tag()?.into_owned()?,
            });
            cursor = cursor.try_cdr()?.into_owned()?;
        }
        Ok(Self(arguments))
    }
    fn matched(&self) -> SexpResult<[Option<Sexp<'static>>; 5]> {
        const FORMALS: [&str; 5] = ["x", "envir", "mode", "ifnotfound", "inherits"];
        let mut result = [const { None }; 5];
        let mut used = vec![false; self.0.len()];
        let tags = self
            .0
            .iter()
            .map(|arg| {
                if arg.tag.is_nil() {
                    Ok(None)
                } else {
                    Ok(Some(arg.tag.try_printname()?.try_as_string()?))
                }
            })
            .collect::<SexpResult<Vec<_>>>()?;
        // GNU closure matching resolves exact names before partial names and positions.
        for (index, tag) in tags.iter().enumerate() {
            if let Some(position) = tag
                .as_ref()
                .and_then(|tag| FORMALS.iter().position(|name| name == tag))
            {
                if result[position].is_some() {
                    return Err(failure("formal argument matched multiple times"));
                }
                result[position] = Some(self.0[index].value.clone());
                used[index] = true;
            }
        }
        for (index, tag) in tags.iter().enumerate() {
            if used[index] || tag.is_none() {
                continue;
            }
            let tag = tag.as_ref().unwrap();
            let candidates = FORMALS
                .iter()
                .enumerate()
                .filter(|(_, name)| name.starts_with(tag))
                .map(|(i, _)| i)
                .collect::<Vec<_>>();
            if candidates.len() != 1 {
                return Err(failure("unused or ambiguous mget argument"));
            }
            let position = candidates[0];
            if result[position].is_some() {
                return Err(failure("formal argument matched multiple times"));
            }
            result[position] = Some(self.0[index].value.clone());
            used[index] = true;
        }
        let mut position = 0;
        for (index, arg) in self.0.iter().enumerate() {
            if used[index] {
                continue;
            }
            while position < result.len() && result[position].is_some() {
                position += 1;
            }
            if position == result.len() {
                return Err(failure("unused mget argument"));
            }
            result[position] = Some(arg.value.clone());
            position += 1;
        }
        Ok(result)
    }
}

fn children(value: &Sexp<'static>, access: &RuntimeAccess) -> SexpResult<Vec<Sexp<'static>>> {
    let length = usize::try_from(value.len()).map_err(|_| failure("invalid mget vector length"))?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(length)
        .map_err(|_| failure("mget input allocation failed"))?;
    for index in 0..length {
        values.push(value.try_vector_elt(index as R_xlen_t)?.into_owned()?);
        access.require_active()?;
    }
    Ok(values)
}

// Native callbacks may unwind after closing the original owner. Recheck before
// propagating a live owner's exact signal, and deny publication on revocation.
fn callback<T>(access: &RuntimeAccess, operation: impl FnOnce() -> SexpResult<T>) -> SexpResult<T> {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation));
    access.require_active()?;
    match result {
        Ok(value) => value,
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

#[derive(Clone, Copy)]
struct Mode {
    kind: SEXPTYPE,
    s4: bool,
}
impl Mode {
    fn parse(text: &str) -> SexpResult<Self> {
        let kind = match text {
            "any" => SEXPTYPE::ANYSXP,
            "NULL" => SEXPTYPE::NILSXP,
            "symbol" | "name" => SEXPTYPE::SYMSXP,
            "pairlist" => SEXPTYPE::LISTSXP,
            "closure" | "function" | "builtin" | "special" => SEXPTYPE::CLOSXP,
            "environment" => SEXPTYPE::ENVSXP,
            "promise" => SEXPTYPE::PROMSXP,
            "language" => SEXPTYPE::LANGSXP,
            "char" => SEXPTYPE::CHARSXP,
            "logical" => SEXPTYPE::LGLSXP,
            "integer" | "numeric" | "double" => SEXPTYPE::REALSXP,
            "complex" => SEXPTYPE::CPLXSXP,
            "character" => SEXPTYPE::STRSXP,
            "..." => SEXPTYPE::DOTSXP,
            "list" => SEXPTYPE::VECSXP,
            "expression" => SEXPTYPE::EXPRSXP,
            "bytecode" => SEXPTYPE::BCODESXP,
            "externalptr" => SEXPTYPE::EXTPTRSXP,
            "weakref" => SEXPTYPE::WEAKREFSXP,
            "raw" => SEXPTYPE::RAWSXP,
            "S4" | "object" => SEXPTYPE::OBJSXP,
            _ => return Err(failure(format!("invalid 'mode' argument '{text}'"))),
        };
        Ok(Self {
            kind,
            s4: text == "S4",
        })
    }
    fn matches(self, value: &Sexp<'_>, access: &RuntimeAccess) -> SexpResult<bool> {
        let kind = match value.typeof_() {
            SEXPTYPE::INTSXP => SEXPTYPE::REALSXP,
            SEXPTYPE::BUILTINSXP | SEXPTYPE::SPECIALSXP => SEXPTYPE::CLOSXP,
            kind => kind,
        };
        if kind != self.kind {
            return Ok(false);
        }
        if kind == SEXPTYPE::OBJSXP {
            return access.with_native(|_| {
                Ok(
                    (unsafe { crate::mainutils::coerce::IS_S4_OBJECT(value.as_raw()) } != 0)
                        == self.s4,
                )
            });
        }
        Ok(true)
    }
}

fn force(value: Sexp<'static>, access: &RuntimeAccess) -> SexpResult<Sexp<'static>> {
    callback(access, || {
        access.with_native(|_| {
            unsafe { crate::sexp::envir::force_promise_result(value) }
                .map_err(failure)?
                .ok_or_else(|| failure("value not found"))?
                .into_owned()
        })
    })
}

fn lookup(
    symbol: &Sexp<'static>,
    mut environment: Sexp<'static>,
    mode: Mode,
    inherits: bool,
    access: &RuntimeAccess,
) -> SexpResult<Option<Sexp<'static>>> {
    let mut seen = std::collections::HashSet::new();
    while environment.typeof_() == SEXPTYPE::ENVSXP {
        let identity = environment
            .allocation()?
            .link()
            .ok_or(SexpError::StaleAllocation)?;
        seen.try_reserve(1)
            .map_err(|_| failure("mget environment allocation failed"))?;
        if !seen.insert(identity) {
            return Err(failure("cyclic mget environment chain"));
        }
        let selected = callback(access, || {
            access.with_native(|_| {
                unsafe {
                    crate::sexp::envir::find_var_in_frame_result(
                        environment.clone(),
                        symbol.clone(),
                    )
                }
                .map_err(failure)?
                .map(Sexp::into_owned)
                .transpose()
            })
        })?;
        if let Some(value) = selected {
            if mode.kind == SEXPTYPE::ANYSXP {
                return Ok(Some(force(value, access)?));
            }
            let value = force(value, access)?;
            if mode.matches(&value, access)? {
                return Ok(Some(value));
            }
        }
        if !inherits {
            break;
        }
        environment = environment.try_enclos()?.into_owned()?;
    }
    Ok(None)
}

fn execute(
    arguments: Arguments,
    caller: Sexp<'static>,
    access: &RuntimeAccess,
) -> SexpResult<Sexp<'static>> {
    let domain = access.domain();
    let [x, environment, modes, fallback, inherits] = arguments.matched()?;
    let x = x.ok_or_else(|| failure("invalid first argument"))?;
    if x.typeof_() != SEXPTYPE::STRSXP {
        return Err(failure("invalid first argument"));
    }
    let length = x.len();
    access.require_active()?;
    let mut names = Vec::new();
    names
        .try_reserve_exact(
            usize::try_from(length).map_err(|_| failure("invalid mget names length"))?,
        )
        .map_err(|_| failure("mget names allocation failed"))?;
    for index in 0..length {
        let character = x.try_string_elt(index)?.into_owned()?;
        access.require_active()?;
        let name = character.try_as_string()?;
        if name.is_empty() {
            return Err(failure(format!("invalid name in position {}", index + 1)));
        }
        names.push(name);
    }
    let environment = environment.unwrap_or_else(|| caller.clone());
    if environment.is_nil() {
        return Err(failure("use of NULL environment is defunct"));
    }
    if environment.typeof_() != SEXPTYPE::ENVSXP {
        return Err(failure("second argument must be an environment"));
    }
    let modes = match modes {
        Some(value) => value,
        None => access.allocator(&domain)?.strings(&["any"])?.into_owned()?,
    };
    if modes.typeof_() != SEXPTYPE::STRSXP {
        return Err(failure("invalid 'mode' argument"));
    }
    let mode_length = modes.len();
    access.require_active()?;
    if mode_length != 1 && mode_length != length {
        return Err(failure("wrong length for 'mode' argument"));
    }
    // Every matched parent is already owned; expose children only after GNU
    // names/environment/mode validation, in the original provider order.
    let fallbacks = match fallback {
        Some(value) if value.typeof_() == SEXPTYPE::VECSXP => Some(children(&value, access)?),
        Some(value) => {
            let list = callback(access, || {
                access.with_native(|token| unsafe {
                    token
                        .sexp(crate::mainutils::coerce::coerceVector(
                            value.as_raw(),
                            SEXPTYPE::VECSXP.as_c_int(),
                        ))?
                        .into_owned()
                })
            })?;
            Some(children(&list, access)?)
        }
        None => None,
    };
    if let Some(fallbacks) = &fallbacks
        && fallbacks.len() != 1
        && fallbacks.len() != length as usize
    {
        return Err(failure("wrong length for 'ifnotfound' argument"));
    }
    let inherits = if let Some(value) = inherits {
        let flag = callback(access, || {
            access
                .with_native(|_| Ok(unsafe { crate::mainutils::coerce::asLogical(value.as_raw()) }))
        })?;
        if flag == NA_INTEGER {
            return Err(failure("invalid 'inherits' argument"));
        }
        flag != 0
    } else {
        false
    };
    let allocator = access.allocator(&domain)?;
    let result = allocator
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::VECSXP, length)))?
        .into_owned()?;
    let names_value = x.clone();
    let mut output = SexpMut::try_from_checked(result)?;
    for (index, name) in names.iter().enumerate() {
        let mode_character = modes
            .try_string_elt(index as R_xlen_t % mode_length)?
            .into_owned()?;
        access.require_active()?;
        let mode = Mode::parse(&mode_character.try_as_string()?)?;
        let symbol = access.with_native(|token| {
            let name =
                std::ffi::CString::new(name.as_str()).map_err(|_| failure("invalid mget name"))?;
            unsafe {
                token
                    .sexp(crate::sexp::symbol::Rf_install(name.as_ptr()))?
                    .into_owned()
            }
        })?;
        let selected = lookup(&symbol, environment.clone(), mode, inherits, access)?;
        let value = match selected {
            Some(value) => value,
            None => {
                let fallback = fallbacks
                    .as_ref()
                    .ok_or_else(|| failure(format!("value for '{name}' not found")))?;
                let value = fallback[index % fallback.len()].clone();
                let value = if matches!(
                    value.typeof_(),
                    SEXPTYPE::CLOSXP | SEXPTYPE::BUILTINSXP | SEXPTYPE::SPECIALSXP
                ) {
                    let argument = allocator.strings(&[name])?.into_owned()?;
                    let list = allocator
                        .pairlist_cell(&argument, &domain.nil(), &domain.nil())?
                        .into_owned()?;
                    let call = allocator.call(&value, &list)?.into_owned()?;
                    callback(access, || {
                        access.with_native(|_| unsafe {
                            crate::eval::eval::eval(call, caller.clone())
                                .map_err(failure)?
                                .into_owned()
                        })
                    })?
                } else {
                    value
                };
                force(value, access)?
            }
        };
        access.with_native(|_| {
            unsafe {
                crate::mainutils::duplicate::lazy_duplicate(value.as_raw());
            }
            Ok(())
        })?;
        access.require_active()?;
        output.try_set_vector_elt(index as R_xlen_t, value)?;
    }
    let result = output.freeze();
    access.with_native(|_| {
        unsafe {
            crate::mainutils::duplicate::lazy_duplicate(names_value.as_raw());
            crate::sexp::attrib_core::setAttrib(
                result.as_raw(),
                crate::sexp::attrib_core::R_NamesSymbol(),
                names_value.as_raw(),
            );
        }
        Ok(())
    })?;
    access.require_active()?;
    Ok(result)
}

/// # Safety
/// Arguments belong to the live current runtime at this translated entry.
pub(super) unsafe fn invoke(args: SEXP, rho: SEXP) -> SexpResult<Sexp<'static>> {
    let token = unsafe { OwnerToken::current()? };
    let owner = token.weak_owner().ok_or(SexpError::RootUnavailable)?;
    with_runtime(&owner, |access| {
        let arguments = Arguments::capture(access.domain().wrap(args)?.into_owned()?)?;
        let caller = access.domain().wrap(rho)?.into_owned()?;
        callback(access, || execute(arguments, caller, access))
    })?
}
