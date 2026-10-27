//! Public positions and names are distinct from stable callback iteration identities.
use super::*;
use crate::sexp::{
    memory::WorkspaceReservation,
    object::{Sexp, SexpError, SexpMut, SexpResult},
    owner::RuntimeAccess,
};

#[path = "task_callback_registration/wrappers.rs"]
mod wrappers;

fn allocation() -> SexpError {
    SexpError::AllocationFailed {
        object: "task callback storage",
    }
}

fn reserve_slot(access: &RuntimeAccess) -> SexpResult<()> {
    let (len, capacity) = access.with_native(|owner| {
        Ok(unsafe {
            let s = &(*owner.as_ptr()).main_state;
            (s.task_callbacks.len(), s.task_callbacks.capacity())
        })
    })?;
    if len < capacity {
        return Ok(());
    }
    let needed = len.checked_add(1).ok_or_else(allocation)?;
    let count = capacity.checked_mul(2).unwrap_or(needed).max(needed).max(4);
    let bytes = count
        .checked_mul(std::mem::size_of::<ToplevelTaskCallback>())
        .ok_or_else(allocation)?;
    let mut ticket = access
        .with_arena(|a| a.reserve_workspace(bytes))?
        .ok_or_else(allocation)?;
    let mut values = Vec::new();
    values.try_reserve_exact(count).map_err(|_| allocation())?;
    ticket
        .try_resize(
            values
                .capacity()
                .checked_mul(std::mem::size_of::<ToplevelTaskCallback>())
                .ok_or_else(allocation)?,
        )
        .ok_or_else(allocation)?;
    let (old, old_ticket) = access.with_native(|owner| {
        let state = unsafe { &mut (*owner.as_ptr()).main_state };
        if state.task_callbacks.len() >= values.capacity() {
            return Err(allocation());
        }
        values.append(&mut state.task_callbacks);
        Ok((
            std::mem::replace(&mut state.task_callbacks, values),
            state.task_callback_workspace.replace(ticket),
        ))
    })?;
    drop(old);
    drop(old_ticket);
    Ok(())
}

pub(super) struct Registration {
    pub(super) id: c_int,
    pub(super) position: c_int,
    name: Sexp<'static>,
}

pub(super) fn register(
    access: &RuntimeAccess,
    fun: Sexp<'static>,
    data: Sexp<'static>,
    use_data: bool,
    supplied: Option<&Sexp<'_>>,
) -> SexpResult<Registration> {
    if !fun.is_function() {
        return Err(SexpError::EvaluationFailed {
            message: "task callback must be a function".into(),
        });
    }
    let custom = match supplied {
        Some(name) if name.len() > 0 => Some(name.try_string_elt(0)?.into_owned()?),
        _ => None,
    };
    loop {
        access.with_native(|owner| unsafe {
            (*owner.as_ptr())
                .main_state
                .next_task_callback_id
                .checked_add(1)
                .ok_or_else(allocation)
        })?;
        reserve_slot(access)?;
        let (previous, position) = access.with_native(|owner| {
            let s = unsafe { &(*owner.as_ptr()).main_state };
            let position = c_int::try_from(s.task_callbacks.len())
                .ok()
                .and_then(|n| n.checked_add(1))
                .ok_or_else(allocation)?;
            Ok((s.next_task_callback_id, position))
        })?;
        let name = if let Some(name) = &custom {
            name.clone()
        } else {
            // Format the bounded integer into stack storage; only the resulting
            // CHARSXP backing is retained, through its ordinary owning handle.
            use std::io::Write;
            let mut storage = [0u8; 24];
            let length = {
                let mut writer = std::io::Cursor::new(storage.as_mut_slice());
                write!(&mut writer, "{position}").map_err(|_| allocation())?;
                writer.position() as usize
            };
            let text = std::str::from_utf8(&storage[..length]).map_err(|_| allocation())?;
            access
                .allocator(&access.domain())?
                .character(text)?
                .into_owned()?
        };
        let entry = access.with_native(|owner| {
            let s = unsafe { &mut (*owner.as_ptr()).main_state };
            // Name allocation can collect/reenter. Restart without publishing
            // when a callback has changed the original registry in that gap.
            if s.next_task_callback_id != previous
                || s.task_callbacks.len() != position as usize - 1
            {
                return Ok(None);
            }
            let id = previous.checked_add(1).ok_or_else(allocation)?;
            if s.task_callbacks.len() == s.task_callbacks.capacity() {
                return Ok(None);
            }
            s.task_callbacks.push(ToplevelTaskCallback {
                id,
                name: name.clone(),
                use_data,
                fun: fun.clone(),
                data: data.clone(),
            });
            s.next_task_callback_id = id;
            Ok(Some(Registration {
                id,
                position,
                name: name.clone(),
            }))
        })?;
        if let Some(entry) = entry {
            return Ok(entry);
        }
    }
}

fn bridge(operation: impl FnOnce(&RuntimeAccess) -> SexpResult<Sexp<'static>>) -> SEXP {
    let owner = task_callback_owner();
    crate::sexp::owner::with_runtime(&owner, |access| {
        let value = operation(access)?;
        access.require_active()?;
        Ok(value)
    })
    .and_then(|r| r)
    .unwrap_or_else(|e| main_error(e.to_string()))
    .as_raw()
}

pub(crate) unsafe extern "C-unwind" fn add(
    fun: SEXP,
    data: SEXP,
    use_data: SEXP,
    name: SEXP,
) -> SEXP {
    bridge(|access| {
        let domain = access.domain();
        let fun = domain.wrap(fun)?.into_owned()?;
        let data = domain.wrap(data)?.into_owned()?;
        let used = domain.wrap(use_data)?.try_logical_elt(0)? != FALSE;
        let name = domain.wrap(name)?.into_owned()?;
        let allocator = access.allocator(&domain)?;
        // GNU creates its result before publishing the registration. A denied
        // result allocation must not leave a newly registered callback behind.
        let result = allocator.allocate(|a| Some(a.alloc_vector(SEXPTYPE::INTSXP, 1)))?;
        let entry = register(access, fun, data, used, Some(&name))?;
        let mut result = SexpMut::try_from_checked(result)?;
        result.try_set_integer_elt(0, entry.position - 1)?;
        let result = result.freeze();
        // The native names assignment follows registration in GNU, so this
        // malformed-name error intentionally retains the registered first name.
        if name.len() > 1 {
            return Err(SexpError::EvaluationFailed {
                message: format!(
                    "'names' attribute [{}] must be the same length as the vector [1]",
                    name.len()
                ),
            });
        }
        let names = if name.len() > 0 {
            name
        } else {
            let names = allocator.allocate(|a| Some(a.alloc_vector(SEXPTYPE::STRSXP, 1)))?;
            let mut names = SexpMut::try_from_checked(names)?;
            names.try_set_string_elt(0, entry.name)?;
            names.freeze().into_owned()?
        };
        access.with_native(|_| {
            unsafe {
                crate::sexp::attrib_core::setAttrib(
                    result.as_raw(),
                    crate::sexp::attrib_core::R_NamesSymbol(),
                    names.as_raw(),
                );
            }
            Ok(())
        })?;
        result.into_owned()
    })
}

pub(super) fn name_position(
    callbacks: &[ToplevelTaskCallback],
    name: &str,
) -> SexpResult<Option<usize>> {
    for (index, callback) in callbacks.iter().enumerate() {
        if callback.name.try_char_eq(name.as_bytes())? {
            return Ok(Some(index));
        }
    }
    Ok(None)
}

pub(crate) unsafe extern "C-unwind" fn remove(which: SEXP) -> SEXP {
    bridge(|access| {
        let which = access.domain().wrap(which)?.into_owned()?;
        // Only the original character handle is retained. No copied name or
        // payload reference crosses the coercion provider or native scopes.
        let name = if which.typeof_() == SEXPTYPE::STRSXP {
            if which.len() == 0 {
                return access.domain().logical(false).into_owned();
            }
            Some(which.try_string_elt(0)?.into_owned()?)
        } else {
            None
        };
        let index = if name.is_none() {
            Some(access.with_native(|_| {
                Ok(unsafe { crate::mainutils::coerce::asInteger(which.as_raw()) })
            })?)
        } else {
            None
        };
        let retired = access.with_native(|owner| {
            let callbacks = unsafe { &mut (*owner.as_ptr()).main_state.task_callbacks };
            let position = if let Some(name) = &name {
                // CHAR and try_char_eq only read immutable character backing;
                // neither can allocate, collect, invoke providers or callbacks.
                let text = unsafe {
                    std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(name.as_raw()))
                }
                .to_bytes();
                let mut found = None;
                for (i, callback) in callbacks.iter().enumerate() {
                    if callback.name.try_char_eq(text)? {
                        found = Some(i);
                        break;
                    }
                }
                found
            } else {
                match index.unwrap() {
                    NA_INTEGER => None,
                    n if n < 1 => {
                        return Err(SexpError::EvaluationFailed {
                            message: "negative index passed to R_removeTaskCallbackByIndex".into(),
                        });
                    }
                    n => (n as usize - 1 < callbacks.len()).then_some(n as usize - 1),
                }
            };
            Ok(position.map(|i| callbacks.remove(i)))
        })?;
        let found = retired.is_some();
        drop(retired);
        access.domain().logical(found).into_owned()
    })
}

struct NamesSnapshot {
    values: Vec<Sexp<'static>>,
    _ticket: WorkspaceReservation,
}

pub(crate) unsafe extern "C-unwind" fn names() -> SEXP {
    bridge(|access| {
        let count = access.with_native(|owner| {
            Ok(unsafe { (*owner.as_ptr()).main_state.task_callbacks.len() })
        })?;
        let bytes = count
            .checked_mul(std::mem::size_of::<Sexp<'static>>())
            .ok_or_else(allocation)?;
        let mut ticket = access
            .with_arena(|a| a.reserve_workspace(bytes))?
            .ok_or_else(allocation)?;
        let mut values = Vec::new();
        values.try_reserve_exact(count).map_err(|_| allocation())?;
        ticket
            .try_resize(
                values
                    .capacity()
                    .checked_mul(std::mem::size_of::<Sexp<'static>>())
                    .ok_or_else(allocation)?,
            )
            .ok_or_else(allocation)?;
        access.with_native(|owner| {
            let callbacks = unsafe { &(*owner.as_ptr()).main_state.task_callbacks };
            if callbacks.len() != count {
                return Err(allocation());
            }
            for c in callbacks {
                values.push(c.name.clone());
            }
            Ok(())
        })?;
        let snapshot = NamesSnapshot {
            values,
            _ticket: ticket,
        };
        let allocator = access.allocator(&access.domain())?;
        let result = allocator.allocate(|a| Some(a.alloc_vector(SEXPTYPE::STRSXP, count as _)))?;
        let mut result = SexpMut::try_from_checked(result)?;
        for (i, name) in snapshot.values.iter().enumerate() {
            result.try_set_string_elt(i as _, name.clone())?;
        }
        result.freeze().into_owned()
    })
}

/// Ordinary promises preserve base lookup/locking semantics while retaining
/// only one authenticated loader and three small expressions before first use.
/// No string lookup or foreign loader can obtain this private typed routine.
pub(crate) unsafe fn install(base_env: SEXP) {
    use crate::mainutils::{
        native_routines::{CallRoutine, NativeRoutine},
        native_symbols,
    };
    let owner = unsafe { crate::sexp::owner::OwnerToken::current() }
        .unwrap_or_else(|e| main_error(e.to_string()));
    let _pin = owner.pin().unwrap_or_else(|e| main_error(e.to_string()));
    (|| -> SexpResult<()> {
        let factory = owner.node_factory();
        let target = factory.wrap(base_env)?.into_owned()?;
        let call = factory.wrap(unsafe { Rf_install(c".Call".as_ptr()) })?;
        let loader = native_symbols::symbol_pointer(
            owner,
            NativeRoutine::Call(CallRoutine::Args1(materialize)),
        )?;
        for (binding, selector) in [
            (c".C_R_addTaskCallback", 0),
            (c".C_R_removeTaskCallback", 1),
            (c".C_R_getTaskCallbackNames", 2),
            (c"addTaskCallback", 3),
            (c"removeTaskCallback", 4),
            (c"getTaskCallbackNames", 5),
        ] {
            let symbol = factory.wrap(unsafe { Rf_install(binding.as_ptr()) })?;
            let index = factory.allocate(|a| {
                crate::sexp::builder::scalar_integer_in(a, selector).map(|v| v.as_raw())
            })?;
            let arguments = factory.pairlist_cell(&index, &factory.nil(), &factory.nil())?;
            let arguments = factory.pairlist_cell(&loader, &arguments, &factory.nil())?;
            let expression = factory.call(&call, &arguments)?;
            let promise = factory.promise(&expression, &target)?;
            owner.require_active()?;
            if !unsafe {
                crate::sexp::envir::define_var_safe(symbol.clone(), promise.clone(), target.clone())
            } {
                return Err(SexpError::EvaluationFailed {
                    message: "cannot install task callback registration".into(),
                });
            }
            if selector >= 3 {
                owner.require_active()?;
                unsafe {
                    crate::sexp::accessors::SET_SYMVALUE(symbol.as_raw(), promise.as_raw());
                }
            }
        }
        Ok(())
    })()
    .unwrap_or_else(|e| main_error(e.to_string()));
}

unsafe extern "C-unwind" fn materialize(index: SEXP) -> SEXP {
    use crate::mainutils::{
        native_routines::{CallRoutine, NativeRoutine},
        native_symbols,
    };
    let owner = unsafe { crate::sexp::owner::OwnerToken::current() }
        .unwrap_or_else(|e| main_error(e.to_string()));
    let _pin = owner.pin().unwrap_or_else(|e| main_error(e.to_string()));
    (|| -> SexpResult<Sexp<'static>> {
        let factory = owner.node_factory();
        let index = factory.wrap(index)?.try_integer_elt(0)?;
        owner.require_active()?;
        if (3..=5).contains(&index) {
            return wrappers::materialize(owner, index);
        }
        let (name, routine) = match index {
            0 => ("R_addTaskCallback", CallRoutine::Args4(add)),
            1 => ("R_removeTaskCallback", CallRoutine::Args1(remove)),
            2 => ("R_getTaskCallbackNames", CallRoutine::Args0(names)),
            _ => {
                return Err(SexpError::EvaluationFailed {
                    message: "invalid task callback registration".into(),
                });
            }
        };
        {
            let factory = owner.node_factory();
            let keys = native_symbols::AttributeKeys {
                names: factory.wrap(unsafe { Rf_install(c"names".as_ptr()) })?,
                class: factory.wrap(unsafe { Rf_install(c"class".as_ptr()) })?,
            };
            native_symbols::registered_routine_info(
                owner,
                name,
                "base",
                true,
                NativeRoutine::Call(routine),
                &keys,
            )
        }
    })()
    .unwrap_or_else(|e| main_error(e.to_string()))
    .as_raw()
}
