#![forbid(unsafe_code)]
//! Allocation-bound authority for the statically linked Rust registrations.
//! Display metadata and external-pointer address bits grant no authority.
use super::native_routines::{NativeRoutine, PayloadArity};
use crate::sexp::{
    ffi::{SEXP, SEXPTYPE},
    memory::WorkspaceReservation,
    object::{SessionNodeFactory, Sexp, SexpError, SexpMut, SexpResult},
    owner::OwnerToken,
};
use std::{cell::Cell, rc::Rc};

pub(crate) const PACKAGES: &[&str] = &[
    "methods",
    "tools",
    "utils",
    "stats",
    "splines",
    "grDevices",
    "grid",
    "graphics",
];

pub(crate) struct AttributeKeys<'s> {
    pub(crate) names: Sexp<'s>,
    pub(crate) class: Sexp<'s>,
}

pub(crate) fn invalid_symbol() -> SexpError {
    SexpError::EvaluationFailed {
        message: "first argument must be a string (of length 1) or native symbol reference".into(),
    }
}

fn memory_error() -> SexpError {
    SexpError::EvaluationFailed {
        message: "native symbol metadata exceeds session memory limit".into(),
    }
}

struct LibraryHandle {
    package: &'static str,
    active: Cell<bool>,
    _ticket: WorkspaceReservation,
}

struct SymbolHandle {
    routine: NativeRoutine,
    active: Cell<bool>,
    _ticket: WorkspaceReservation,
}

pub(crate) struct SymbolAuthority {
    resource: Rc<SymbolHandle>,
    address: Sexp<'static>,
}

impl SymbolAuthority {
    pub(crate) fn routine(&self) -> SexpResult<NativeRoutine> {
        self.address.node_factory()?.require_active()?;
        if !self.resource.active.get() {
            return Err(invalid_symbol());
        }
        Ok(self.resource.routine)
    }
}

pub(crate) struct LibraryAuthority {
    resource: Rc<LibraryHandle>,
    address: Sexp<'static>,
}

impl LibraryAuthority {
    pub(crate) fn package(&self) -> SexpResult<&'static str> {
        self.address.node_factory()?.require_active()?;
        if !self.resource.active.get() {
            return Err(invalid_symbol());
        }
        Ok(self.resource.package)
    }
}

fn ticket<T>(owner: OwnerToken<'_>) -> SexpResult<WorkspaceReservation> {
    let bytes = std::mem::size_of::<T>()
        .checked_add(2 * std::mem::size_of::<usize>())
        .ok_or_else(invalid_symbol)?;
    owner
        .with_arena(|a| a.reserve_workspace(bytes))?
        .ok_or_else(memory_error)
}

fn pointer<T: 'static>(
    factory: &SessionNodeFactory<'_>,
    resource: Rc<T>,
) -> SexpResult<Sexp<'static>> {
    factory
        .allocate(|arena| {
            let value = arena.alloc_node(SEXPTYPE::EXTPTRSXP);
            let node = arena.node_token(value)?;
            arena.heap_identity().attach_resource(&node, resource)?;
            Some(value)
        })?
        .into_owned()
}

fn resource<T: 'static>(owner: OwnerToken<'_>, value: &Sexp<'_>) -> SexpResult<Option<Rc<T>>> {
    owner.require_active()?;
    owner.node_factory().link(value)?;
    if value.typeof_() != SEXPTYPE::EXTPTRSXP {
        return Ok(None);
    }
    let node = value.allocation()?;
    Ok(node.heap_identity().resource::<T>(node))
}

pub(crate) fn symbol(owner: OwnerToken<'_>, value: &Sexp<'_>) -> SexpResult<SymbolAuthority> {
    let resource = resource::<SymbolHandle>(owner, value)?.ok_or_else(invalid_symbol)?;
    let authority = SymbolAuthority {
        resource,
        address: value.clone().into_owned()?,
    };
    authority.routine()?;
    Ok(authority)
}

pub(crate) fn library(owner: OwnerToken<'_>, value: &Sexp<'_>) -> SexpResult<LibraryAuthority> {
    let resource = resource::<LibraryHandle>(owner, value)?.ok_or_else(invalid_symbol)?;
    let authority = LibraryAuthority {
        resource,
        address: value.clone().into_owned()?,
    };
    authority.package()?;
    Ok(authority)
}

/// Compatibility entry already owns the raw argument; this performs no R
/// allocation, callback, or collection and never changes unrelated resources.
pub(crate) fn invalidate(owner: OwnerToken<'_>, value: SEXP) -> SexpResult<()> {
    let handles = owner.with_arena(|arena| {
        let node = arena.node_token(value)?;
        let heap = arena.heap_identity();
        Some((
            heap.resource::<LibraryHandle>(&node),
            heap.resource::<SymbolHandle>(&node),
        ))
    })?;
    if let Some((library, symbol)) = handles {
        if let Some(handle) = library {
            handle.active.set(false);
        }
        if let Some(handle) = symbol {
            handle.active.set(false);
        }
    }
    Ok(())
}
pub(crate) fn lookup(
    name: &str,
    package: Option<&str>,
) -> Option<crate::mainutils::native_routines::NativeRoutine> {
    fn in_package(
        name: &str,
        package: &str,
    ) -> Option<crate::mainutils::native_routines::NativeRoutine> {
        match package {
            "methods" => crate::library::methods::native_calls::lookup(name),
            "tools" => crate::library::tools::native_calls::lookup(name),
            "utils" => crate::library::utils::lookup(name),
            "stats" => crate::library::stats::random::lookup_external(name)
                .or_else(|| crate::library::stats::random::lookup_call(name)),
            "splines" => crate::library::splines::splines::lookup(name),
            "grDevices" => crate::library::grdevices::lookup(name),
            "grid" => crate::library::grid::lookup(name),
            "graphics" => crate::library::graphics::lookup(name),
            _ => None,
        }
    }
    match package {
        Some(package) => in_package(name, package),
        None => [
            "methods",
            "tools",
            "utils",
            "stats",
            "splines",
            "grDevices",
            "grid",
            "graphics",
        ]
        .into_iter()
        .find_map(|package| in_package(name, package)),
    }
}

fn decorate(
    factory: &SessionNodeFactory<'_>,
    value: Sexp<'static>,
    keys: &AttributeKeys<'_>,
    names: Option<&[&str]>,
    classes: &[&str],
) -> SexpResult<Sexp<'static>> {
    let class = factory.strings(classes)?;
    let mut attributes = factory.pairlist_cell(&class, &factory.nil(), &keys.class)?;
    if let Some(names) = names {
        let names = factory.strings(names)?;
        attributes = factory.pairlist_cell(&names, &attributes, &keys.names)?;
    }
    let mut value = SexpMut::try_from_checked(value)?;
    value.try_set_attribute(&attributes)?;
    let value = value.freeze();
    let node = value.allocation()?;
    let heap = node.heap_identity();
    let mut header = heap.node_snapshot(node).ok_or(SexpError::StaleAllocation)?;
    header.sxpinfo.set_obj(true);
    heap.replace_node(node, header)
        .ok_or(SexpError::StaleAllocation)?;
    Ok(value)
}

fn list(
    factory: &SessionNodeFactory<'_>,
    values: &[Sexp<'_>],
    keys: &AttributeKeys<'_>,
    names: &[&str],
    classes: &[&str],
) -> SexpResult<Sexp<'static>> {
    let value = factory.allocate(|a| Some(a.alloc_vector(SEXPTYPE::VECSXP, values.len() as _)))?;
    let mut value = SexpMut::try_from_checked(value)?;
    for (index, element) in values.iter().enumerate() {
        value.try_set_vector_elt(index as _, element.clone())?;
    }
    decorate(
        factory,
        value.freeze().into_owned()?,
        keys,
        Some(names),
        classes,
    )
}

pub(crate) fn dll_info(
    owner: OwnerToken<'_>,
    package: &'static str,
    keys: &AttributeKeys<'_>,
) -> SexpResult<Sexp<'static>> {
    let _pin = owner.pin()?;
    owner.require_active()?;
    let factory = owner.node_factory();
    let info = pointer(
        &factory,
        Rc::new(LibraryHandle {
            package,
            active: Cell::new(true),
            _ticket: ticket::<LibraryHandle>(owner)?,
        }),
    )?;
    let info = decorate(&factory, info, keys, None, &["DLLInfoReference"])?;
    // A bundled Rust provider has no host loader address. The opaque reference
    // identifies only its typed registration; it grants no dlsym capability.
    let name = factory.strings(&[package])?;
    let path = factory.strings(&["<in-process Rust registration>"])?;
    let dynamic = factory.domain().logical(false);
    list(
        &factory,
        &[name, path, dynamic, factory.nil(), info],
        keys,
        &["name", "path", "dynamicLookup", "handle", "info"],
        &["DLLInfo"],
    )
}

pub(crate) fn dll_table(
    owner: OwnerToken<'_>,
    keys: &AttributeKeys<'_>,
) -> SexpResult<Sexp<'static>> {
    let _pin = owner.pin()?;
    owner.require_active()?;
    let factory = owner.node_factory();
    let bytes = PACKAGES
        .len()
        .checked_mul(std::mem::size_of::<Sexp<'static>>())
        .ok_or_else(invalid_symbol)?;
    let mut reservation = owner
        .with_arena(|a| a.reserve_workspace(bytes))?
        .ok_or_else(memory_error)?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(PACKAGES.len())
        .map_err(|_| memory_error())?;
    reservation
        .try_resize(values.capacity() * std::mem::size_of::<Sexp<'static>>())
        .ok_or_else(memory_error)?;
    // The small fixed catalogue contains the statically linked Rust providers,
    // not a discovered filesystem library or an arbitrary native address.
    for package in PACKAGES {
        values.push(dll_info(owner, package, keys)?);
    }
    list(&factory, &values, keys, PACKAGES, &["DLLInfoList"])
}

pub(crate) fn symbol_pointer(
    owner: OwnerToken<'_>,
    routine: NativeRoutine,
) -> SexpResult<Sexp<'static>> {
    let resource = Rc::new(SymbolHandle {
        routine,
        active: Cell::new(true),
        _ticket: ticket::<SymbolHandle>(owner)?,
    });
    pointer(&owner.node_factory(), resource)
}

#[cfg(test)]
pub(crate) fn test_symbol_pointer(
    owner: OwnerToken<'_>,
    routine: NativeRoutine,
) -> SexpResult<Sexp<'static>> {
    symbol_pointer(owner, routine)
}

pub(crate) fn symbol_info(
    owner: OwnerToken<'_>,
    name: &str,
    package: &'static str,
    registered: bool,
    keys: &AttributeKeys<'_>,
) -> SexpResult<Option<Sexp<'static>>> {
    let _pin = owner.pin()?;
    owner.require_active()?;
    let Some(routine) = lookup(name, Some(package)) else {
        return Ok(None);
    };
    registered_routine_info(owner, name, package, registered, routine, keys).map(Some)
}

/// Trusted bundled bindings can expose hidden static registrations without
/// making their names available to foreign/dynamic symbol lookup.
pub(crate) fn registered_routine_info(
    owner: OwnerToken<'_>,
    name: &str,
    package: &'static str,
    registered: bool,
    routine: NativeRoutine,
    keys: &AttributeKeys<'_>,
) -> SexpResult<Sexp<'static>> {
    let _pin = owner.pin()?;
    owner.require_active()?;
    let factory = owner.node_factory();
    let pointer = symbol_pointer(owner, routine)?;
    let pointer = decorate(
        &factory,
        pointer,
        keys,
        None,
        &[if registered {
            "RegisteredNativeSymbol"
        } else {
            "NativeSymbol"
        }],
    )?;
    let name = factory.strings(&[name])?;
    let dll = dll_info(owner, package, keys)?;
    let count = match routine.payload_arity() {
        PayloadArity::Fixed(n) => n as i32,
        PayloadArity::Variadic => -1,
    };
    let arity = factory.allocate(|a| Some(a.alloc_vector(SEXPTYPE::INTSXP, 1)))?;
    let mut arity = SexpMut::try_from_checked(arity)?;
    arity.try_set_integer_elt(0, count)?;
    let kind = match routine.interface() {
        super::native_routines::NativeInterface::Call => "CallRoutine",
        _ => "ExternalRoutine",
    };
    list(
        &factory,
        &[name, pointer, dll, arity.freeze()],
        keys,
        &["name", "address", "dll", "numParameters"],
        &[kind, "NativeSymbolInfo"],
    )
}

pub(crate) fn address(value: &Sexp<'_>) -> SexpResult<Option<Sexp<'static>>> {
    match value.typeof_() {
        SEXPTYPE::EXTPTRSXP => value.clone().into_owned().map(Some),
        SEXPTYPE::VECSXP => {
            if value.len() < 2 {
                return Err(invalid_symbol());
            }
            // Own the actual authority child before control providers can detach it.
            let address = value.try_vector_elt(1)?.into_owned()?;
            if address.typeof_() != SEXPTYPE::EXTPTRSXP {
                return Err(invalid_symbol());
            }
            Ok(Some(address))
        }
        _ => Ok(None),
    }
}

pub(crate) struct SymbolName {
    text: String,
    _ticket: WorkspaceReservation,
}
impl SymbolName {
    pub(crate) fn as_str(&self) -> &str {
        &self.text
    }
}

/// Copy the immutable character name only after bounded original-owner admission.
pub(crate) fn name(owner: OwnerToken<'_>, character: &Sexp<'_>) -> SexpResult<SymbolName> {
    let _pin = owner.pin()?;
    owner.require_active()?;
    owner.node_factory().link(character)?;
    let header = character.header();
    if header.type_of() != SEXPTYPE::CHARSXP {
        return Err(invalid_symbol());
    }
    let crate::sexp::ffi::NodeBody::Vector(vector) = header.body else {
        return Err(invalid_symbol());
    };
    let length = usize::try_from(vector.length).map_err(|_| invalid_symbol())?;
    if length >= 1024 {
        return Err(SexpError::EvaluationFailed {
            message: "symbol name is too long".into(),
        });
    }
    let mut ticket = owner
        .with_arena(|a| a.reserve_workspace(length))?
        .ok_or_else(memory_error)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| memory_error())?;
    ticket
        .try_resize(bytes.capacity())
        .ok_or_else(memory_error)?;
    if length != 0 {
        let lease = header.payload_lease().ok_or_else(invalid_symbol)?;
        for index in 0..length {
            bytes.push(lease.byte_elt(index).ok_or_else(invalid_symbol)?);
        }
    }
    let text = String::from_utf8(bytes).map_err(|_| SexpError::InvalidUtf8)?;
    Ok(SymbolName {
        text,
        _ticket: ticket,
    })
}
