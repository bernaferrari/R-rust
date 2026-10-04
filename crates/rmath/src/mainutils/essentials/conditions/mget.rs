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
    fn named(&self, name: &str) -> SexpResult<Option<Sexp<'static>>> {
        for argument in &self.0 {
            if !argument.tag.is_nil()
                && argument.tag.try_printname()?.try_char_eq(name.as_bytes())?
            {
                return Ok(Some(argument.value.clone()));
            }
        }
        Ok(None)
    }
    fn get(&self, name: &str, position: usize) -> SexpResult<Option<Sexp<'static>>> {
        if let Some(value) = self.named(name)? {
            return Ok(Some(value));
        }
        Ok(self
            .0
            .iter()
            .filter(|a| a.tag.is_nil())
            .nth(position)
            .map(|a| a.value.clone()))
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

fn execute(arguments: Arguments, access: &RuntimeAccess) -> SexpResult<Sexp<'static>> {
    let domain = access.domain();
    let x = arguments
        .get("x", 0)?
        .ok_or_else(|| failure("invalid first argument"))?;
    if x.typeof_() != SEXPTYPE::STRSXP {
        return Err(failure("invalid first argument"));
    }
    let environment = arguments.get("envir", 1)?;
    // Retain actual environment and fallback children before name providers,
    // allocations or any binding evaluation can detach their original lists.
    let environments = match environment {
        Some(value) if value.typeof_() == SEXPTYPE::ENVSXP => vec![value],
        Some(value) if value.typeof_() == SEXPTYPE::VECSXP => children(&value, access)?
            .into_iter()
            .filter(|v| v.typeof_() == SEXPTYPE::ENVSXP)
            .collect(),
        Some(value) if !value.is_nil() => Vec::new(),
        _ => vec![access.with_native(|token| unsafe {
            token
                .sexp(crate::sexp::globals::R_GlobalEnv())?
                .into_owned()
        })?],
    };
    if environments.is_empty() {
        return Err(failure("invalid 'envir' argument"));
    }
    let fallback = arguments.get("ifnotfound", 3)?;
    let fallbacks = match fallback {
        Some(value) if value.typeof_() == SEXPTYPE::VECSXP => children(&value, access)?,
        Some(value) if !value.is_nil() => vec![value],
        _ => Vec::new(),
    };
    let inherits = if let Some(value) = arguments.named("inherits")? {
        let flag = if value.len() == 0 {
            NA_INTEGER
        } else {
            match value.typeof_() {
                SEXPTYPE::LGLSXP => value.try_logical_elt(0)?,
                SEXPTYPE::INTSXP => value.try_integer_elt(0)?,
                SEXPTYPE::REALSXP => value.try_real_elt(0)? as i32,
                _ => NA_INTEGER,
            }
        };
        access.require_active()?;
        flag != NA_INTEGER && flag != 0
    } else {
        false
    };
    let length = x.len();
    let mut names = Vec::new();
    names
        .try_reserve_exact(
            usize::try_from(length).map_err(|_| failure("invalid mget names length"))?,
        )
        .map_err(|_| failure("mget names allocation failed"))?;
    for index in 0..length {
        let character = x.try_string_elt(index)?.into_owned()?;
        access.require_active()?;
        names.push(character.try_as_string()?);
    }
    let allocator = access.allocator(&domain)?;
    let result = allocator
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::VECSXP, length)))?
        .into_owned()?;
    let names_value = allocator
        .strings(&names.iter().map(String::as_str).collect::<Vec<_>>())?
        .into_owned()?;
    let mut output = SexpMut::try_from_checked(result)?;
    for (index, name) in names.iter().enumerate() {
        let symbol = access.with_native(|token| {
            let name =
                std::ffi::CString::new(name.as_str()).map_err(|_| failure("invalid mget name"))?;
            unsafe {
                token
                    .sexp(crate::sexp::symbol::Rf_install(name.as_ptr()))?
                    .into_owned()
            }
        })?;
        let environment = environments[index % environments.len()].clone();
        let selected = access.with_native(|_| {
            let selected = unsafe {
                if inherits {
                    crate::sexp::envir::find_var_result(symbol.clone(), environment.clone())
                } else {
                    crate::sexp::envir::find_var_in_frame_result(
                        environment.clone(),
                        symbol.clone(),
                    )
                }
            }
            .map_err(failure)?;
            selected.map(Sexp::into_owned).transpose()
        })?;
        let value = match selected {
            Some(value) => access.with_native(|_| {
                unsafe { crate::sexp::envir::force_promise_result(value) }
                    .map_err(failure)?
                    .ok_or_else(|| failure(format!("value for '{name}' not found")))?
                    .into_owned()
            })?,
            None if !fallbacks.is_empty() => fallbacks[index % fallbacks.len()].clone(),
            None => return Err(failure(format!("value for '{name}' not found"))),
        };
        access.require_active()?;
        output.try_set_vector_elt(index as R_xlen_t, value)?;
    }
    let result = output.freeze();
    access.with_native(|_| {
        unsafe {
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
pub(super) unsafe fn invoke(args: SEXP) -> SexpResult<Sexp<'static>> {
    let token = unsafe { OwnerToken::current()? };
    let owner = token.weak_owner().ok_or(SexpError::RootUnavailable)?;
    with_runtime(&owner, |access| {
        let arguments = Arguments::capture(access.domain().wrap(args)?.into_owned()?)?;
        execute(arguments, access)
    })?
}
