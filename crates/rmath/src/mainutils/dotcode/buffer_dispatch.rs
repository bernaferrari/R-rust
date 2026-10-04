//! Owning admission and marshalling for bundled .C/.Fortran providers.
//! No kernel receives a pointer into an R vector. The separate foreign-loader
//! branch remains an explicitly unsafe host contract.

use crate::mainutils::native_routines::buffers::{BufferInterface, BufferType, NativeBuffer};
use crate::sexp::{
    ffi::{NA_INTEGER, R_xlen_t, SEXP, SEXPTYPE, SxpInfo},
    object::{NodeAllocator, NodeDomain, Sexp, SexpError, SexpMut, SexpResult},
    owner::{OwnerToken, RuntimeAccess, with_runtime},
};

fn failure(message: impl Into<String>) -> SexpError {
    super::native_admission_error(message)
}

struct Argument {
    value: Sexp<'static>,
    tag: Sexp<'static>,
    attributes: Sexp<'static>,
    info: SxpInfo,
}

struct Operands {
    name: Sexp<'static>,
    lookup_name: Sexp<'static>,
    payload: Vec<Argument>,
    controls: Vec<Argument>,
    package: Option<String>,
    naok: bool,
}

impl Operands {
    fn capture(arguments: Sexp<'static>, access: &RuntimeAccess) -> SexpResult<Self> {
        if arguments.is_nil() {
            return Err(failure("'.NAME' is missing"));
        }
        if !arguments.try_tag()?.is_nil() {
            return Err(failure("the first argument should not be named"));
        }
        let name = arguments.try_car()?.into_owned()?;
        let mut all = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut cursor = arguments.try_cdr()?;
        while !cursor.is_nil() {
            let identity = cursor
                .allocation()?
                .link()
                .ok_or(SexpError::StaleAllocation)?;
            if !seen.insert(identity) {
                return Err(failure("cyclic native argument list"));
            }
            let value = cursor.try_car()?.into_owned()?;
            let tag = cursor.try_tag()?.into_owned()?;
            let attributes = value.try_attrib()?.into_owned()?;
            let info = value.header().sxpinfo;
            all.push(Argument {
                value,
                tag,
                attributes,
                info,
            });
            cursor = cursor.try_cdr()?;
        }
        // A structured list-name provider may itself detach the argument
        // graph. Retain every original payload, tag, and attribute first, then
        // retain the selected lookup child before control providers can run.
        let lookup_name = if name.typeof_() == SEXPTYPE::VECSXP && name.len() > 0 {
            name.try_vector_elt(0)?.into_owned()?
        } else {
            name.clone()
        };
        access.require_active()?;
        // Capture every edge and attribute before any provider can detach the
        // original graph. Character/control coercions can reenter R.
        let mut payload = Vec::new();
        let mut controls = Vec::new();
        let mut package = None;
        let mut naok = false;
        for argument in all {
            let tag = if argument.tag.is_nil() {
                String::new()
            } else {
                argument.tag.try_printname()?.try_as_string()?
            };
            access.require_active()?;
            match tag.as_str() {
                "PACKAGE" => {
                    if argument.value.typeof_() != SEXPTYPE::STRSXP || argument.value.len() != 1 {
                        return Err(failure(
                            "PACKAGE argument must be a single character string",
                        ));
                    }
                    let text = argument.value.try_string_elt(0)?.try_as_string()?;
                    access.require_active()?;
                    package = Some(text.strip_prefix("package:").unwrap_or(&text).to_owned());
                    controls.push(argument);
                }
                "NAOK" => {
                    let flag = access.with_native(|_| {
                        Ok(unsafe { crate::mainutils::coerce::asLogical(argument.value.as_raw()) })
                    })?;
                    access.require_active()?;
                    if flag == NA_INTEGER {
                        return Err(failure("invalid 'NAOK' value"));
                    }
                    naok = flag != 0;
                    controls.push(argument);
                }
                // GNU ignores DUP: result buffers are independent even when
                // it is FALSE. ENCODING does not become a native payload.
                "DUP" | "ENCODING" => controls.push(argument),
                _ => payload.push(argument),
            }
        }
        Ok(Self {
            name,
            lookup_name,
            payload,
            controls,
            package,
            naok,
        })
    }

    fn lookup_name(&self) -> SexpResult<Option<String>> {
        match self.lookup_name.typeof_() {
            SEXPTYPE::STRSXP if self.lookup_name.len() == 1 => {
                Ok(Some(self.lookup_name.try_string_elt(0)?.try_as_string()?))
            }
            SEXPTYPE::SYMSXP => Ok(Some(self.lookup_name.try_printname()?.try_as_string()?)),
            _ => Ok(None),
        }
    }

    fn foreign_list(
        &self,
        allocator: &NodeAllocator<'_, 'static>,
        domain: &NodeDomain<'static>,
    ) -> SexpResult<Sexp<'static>> {
        let mut rest = domain.nil();
        // Reconstruct controls into our own cells. The legacy foreign boundary
        // can prune these cells without changing the caller's argument graph.
        for argument in self.controls.iter().rev().chain(self.payload.iter().rev()) {
            rest = allocator.pairlist_cell(&argument.value, &rest, &argument.tag)?;
        }
        allocator.pairlist_cell(&self.name, &rest, &domain.nil())
    }
}

fn marshal(
    argument: &Argument,
    kind: BufferType,
    naok: bool,
    access: &RuntimeAccess,
) -> SexpResult<NativeBuffer> {
    let n = usize::try_from(argument.value.len())
        .map_err(|_| failure("invalid native vector length"))?;
    match kind {
        BufferType::Integer => {
            let mut data = Vec::new();
            data.try_reserve_exact(n)
                .map_err(|_| failure("native buffer allocation failed"))?;
            for i in 0..n {
                let value = if argument.value.typeof_() == SEXPTYPE::LGLSXP {
                    argument.value.try_logical_elt(i as R_xlen_t)?
                } else {
                    argument.value.try_integer_elt(i as R_xlen_t)?
                };
                access.require_active()?;
                if !naok && value == NA_INTEGER {
                    return Err(failure("NA in foreign function call"));
                }
                data.push(value);
            }
            Ok(NativeBuffer::Integer(data))
        }
        BufferType::Real => {
            let mut data = Vec::new();
            data.try_reserve_exact(n)
                .map_err(|_| failure("native buffer allocation failed"))?;
            for i in 0..n {
                let value = argument.value.try_real_elt(i as R_xlen_t)?;
                access.require_active()?;
                if !naok && !value.is_finite() {
                    return Err(failure("NA/NaN/Inf in foreign function call"));
                }
                data.push(value);
            }
            Ok(NativeBuffer::Real(data))
        }
        BufferType::Character => {
            let mut data = Vec::new();
            data.try_reserve_exact(n)
                .map_err(|_| failure("native buffer allocation failed"))?;
            for i in 0..n {
                let text = argument.value.try_string_value_elt(i as R_xlen_t)?;
                access.require_active()?;
                let text = text.ok_or_else(|| failure("NA string in foreign function call"))?;
                if text.as_bytes().contains(&0) {
                    return Err(failure("embedded NUL in native character argument"));
                }
                let mut bytes = text.into_bytes();
                bytes.push(0);
                data.push(bytes);
            }
            Ok(NativeBuffer::Character(data))
        }
    }
}

fn allocate_vector(
    allocator: &NodeAllocator<'_, 'static>,
    kind: SEXPTYPE,
    n: usize,
) -> SexpResult<Sexp<'static>> {
    let n = R_xlen_t::try_from(n).map_err(|_| failure("native vector length overflow"))?;
    allocator.allocate(|arena| arena.alloc_vector_sexp(kind, n).map(|value| value.as_raw()))
}

/// Attribute edges are published only after the original owner records any
/// old-to-young relationship. Allocation callbacks may promote the parent
/// before the names/attribute list is constructed.
fn install_attributes(
    value: &Sexp<'static>,
    attributes: &Sexp<'static>,
    domain: &NodeDomain<'static>,
    access: &RuntimeAccess,
) -> SexpResult<()> {
    domain.link(value)?;
    let attribute = domain.link(attributes)?;
    access.with_native(|owner| {
        // SAFETY: both independently retained values have just been checked
        // against this original domain. The barrier neither allocates an R
        // object nor calls a provider; no GC-state loan is held here.
        if !unsafe {
            crate::sexp::gengc::write_barrier_in(
                owner.as_ptr(),
                value.as_raw(),
                attributes.as_raw(),
            )
        } {
            return Err(SexpError::AllocationFailed {
                object: "native result attribute barrier",
            });
        }
        let node = value.allocation()?;
        let heap = node.heap_identity();
        let mut core = heap.node_snapshot(node).ok_or(SexpError::StaleAllocation)?;
        core.attrib = attribute;
        heap.replace_node(node, core)
            .ok_or(SexpError::StaleAllocation)
    })
}

fn publish(
    buffer: NativeBuffer,
    source: &Argument,
    allocator: &NodeAllocator<'_, 'static>,
    domain: &NodeDomain<'static>,
    access: &RuntimeAccess,
) -> SexpResult<Sexp<'static>> {
    let output = match buffer {
        NativeBuffer::Integer(data) => {
            let kind = source.info.type_of();
            let output = allocate_vector(allocator, kind, data.len())?;
            // Fresh allocation is exclusively initialized here; no callback
            // observes or aliases the vector before initialization completes.
            let mut mutation = SexpMut::try_from_checked(output)?;
            for (i, value) in data.into_iter().enumerate() {
                if kind == SEXPTYPE::LGLSXP {
                    mutation.try_set_logical_elt(i as R_xlen_t, value)?;
                } else {
                    mutation.try_set_integer_elt(i as R_xlen_t, value)?;
                }
            }
            mutation.freeze()
        }
        NativeBuffer::Real(data) => {
            let output = allocate_vector(allocator, SEXPTYPE::REALSXP, data.len())?;
            let mut mutation = SexpMut::try_from_checked(output)?;
            for (i, value) in data.into_iter().enumerate() {
                mutation.try_set_real_elt(i as R_xlen_t, value)?;
            }
            mutation.freeze()
        }
        NativeBuffer::Character(data) => {
            let text: Vec<_> = data
                .into_iter()
                .map(|bytes| {
                    let end = bytes
                        .iter()
                        .position(|v| *v == 0)
                        .ok_or_else(|| failure("unterminated native character result"))?;
                    String::from_utf8(bytes[..end].to_vec())
                        .map_err(|_| failure("invalid native character result"))
                })
                .collect::<SexpResult<_>>()?;
            let slices: Vec<_> = text.iter().map(String::as_str).collect();
            allocator.strings(&slices)?
        }
    };
    install_attributes(&output, &source.attributes, domain, access)?;
    let node = output.allocation()?;
    let heap = node.heap_identity();
    let mut core = heap.node_snapshot(node).ok_or(SexpError::StaleAllocation)?;
    core.sxpinfo.set_obj(source.info.obj());
    const S4_OBJECT: u16 = 1 << 4;
    core.sxpinfo
        .set_gp((core.sxpinfo.gp() & !S4_OBJECT) | (source.info.gp() & S4_OBJECT));
    core.sxpinfo.set_alt(false);
    heap.replace_node(node, core)
        .ok_or(SexpError::StaleAllocation)?;
    Ok(output)
}

/// The translated raw entry is authenticated immediately against the original
/// managed domain; operation-local authority cannot be stored in provider data.
pub(super) unsafe fn invoke(
    call: SEXP,
    operator: SEXP,
    arguments: SEXP,
    environment: SEXP,
    interface: BufferInterface,
) -> SexpResult<Sexp<'static>> {
    let managed = unsafe { OwnerToken::current()? }
        .weak_owner()
        .ok_or(SexpError::RootUnavailable)?;
    let operator_identity =
        unsafe { crate::eval::primitive::PrimitiveDescriptor::from_raw(operator) }
            .map(|descriptor| (descriptor.name, descriptor.op.typeof_()));
    with_runtime(&managed, |access| {
        let domain = access.domain();
        let call = domain.wrap(call)?.into_owned()?;
        let environment = domain.wrap(environment)?.into_owned()?;
        let operands = Operands::capture(domain.wrap(arguments)?.into_owned()?, access)?;
        let name = operands.lookup_name()?;
        access.require_active()?;
        let routine = name
            .as_deref()
            .and_then(crate::library::tools::native_calls::lookup_buffer);
        let allocator = access.allocator(&domain)?;
        let Some(routine) = routine else {
            if name
                .as_deref()
                .and_then(|name| super::lookup_bundled_native(name, operands.package.as_deref()))
                .is_some()
            {
                return Err(failure(
                    "native routine has a different registered interface",
                ));
            }
            if super::native_extension_policy_enabled() {
                return Err(failure(format!(
                    "{interface} native routine is not implemented by the bundled Rust registry"
                )));
            }
            let list = operands.foreign_list(&allocator, &domain)?;
            let operator = match operator_identity {
                Some((name, kind)) => access.with_native(|owner| {
                    let pointer =
                        unsafe { crate::eval::primitive::make_primitive_binding(name, kind) };
                    owner.sexp(pointer)?.into_owned()
                })?,
                None => return Err(failure("invalid foreign-call operator")),
            };
            access.require_active()?;
            // Host libraries must establish their actual ABI independently;
            // this branch cannot reinterpret any bundled Rust kernel pointer.
            let raw = unsafe {
                super::do_foreign_dotcode(
                    call.as_raw(),
                    operator.as_raw(),
                    list.as_raw(),
                    environment.as_raw(),
                )
            };
            access.require_active()?;
            return domain.wrap(raw)?.into_owned();
        };
        routine
            .validate_request(interface, operands.payload.len())
            .map_err(|e| failure(e.to_string()))?;
        if operands
            .package
            .as_deref()
            .is_some_and(|package| package != routine.package())
        {
            return Err(failure(
                "native routine is not registered in the requested PACKAGE",
            ));
        }
        // Check every type before invoking any ALTREP element provider.
        for (argument, kind) in operands.payload.iter().zip(routine.types()) {
            let valid = match kind {
                BufferType::Integer => matches!(
                    argument.value.typeof_(),
                    SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP
                ),
                BufferType::Real => argument.value.typeof_() == SEXPTYPE::REALSXP,
                BufferType::Character => argument.value.typeof_() == SEXPTYPE::STRSXP,
            };
            if !valid {
                return Err(failure(format!("native argument must have type {kind:?}")));
            }
        }
        let mut buffers = operands
            .payload
            .iter()
            .zip(routine.types())
            .map(|(argument, kind)| marshal(argument, *kind, operands.naok, access))
            .collect::<SexpResult<Vec<_>>>()?;
        access.require_active()?;
        routine
            .invoke(interface, &mut buffers)
            .map_err(|e| failure(e.to_string()))?;
        access.require_active()?;
        let mut outputs = Vec::with_capacity(buffers.len());
        for (buffer, source) in buffers.into_iter().zip(&operands.payload) {
            outputs.push(publish(buffer, source, &allocator, &domain, access)?);
        }
        let result = allocate_vector(&allocator, SEXPTYPE::VECSXP, outputs.len())?;
        let mut mutation = SexpMut::try_from_checked(result)?;
        for (i, output) in outputs.into_iter().enumerate() {
            mutation.try_set_vector_elt(i as R_xlen_t, output)?;
        }
        let result = mutation.freeze();
        if operands.payload.iter().any(|a| !a.tag.is_nil()) {
            let text = operands
                .payload
                .iter()
                .map(|a| {
                    if a.tag.is_nil() {
                        Ok(String::new())
                    } else {
                        a.tag.try_printname()?.try_as_string()
                    }
                })
                .collect::<SexpResult<Vec<_>>>()?;
            let slices: Vec<_> = text.iter().map(String::as_str).collect();
            let names = allocator.strings(&slices)?;
            let tag = access.with_native(|owner| {
                let raw = unsafe { crate::sexp::symbol::Rf_install(c"names".as_ptr()) };
                owner.sexp(raw)?.into_owned()
            })?;
            let attributes = allocator.pairlist_cell(&names, &domain.nil(), &tag)?;
            install_attributes(&result, &attributes, &domain, access)?;
        }
        access.require_active()?;
        Ok(result)
    })?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mainutils::native_routines::buffers;
    use crate::sexp::{RSession, object::SessionNodeFactory};
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    mod holtwinters;
    mod lookup_snapshot_tests;

    fn real(factory: &SessionNodeFactory<'_>, data: &[f64]) -> Sexp<'static> {
        let output = factory
            .allocate(|arena| {
                arena
                    .alloc_vector_sexp(SEXPTYPE::REALSXP, data.len() as R_xlen_t)
                    .map(|x| x.as_raw())
            })
            .unwrap()
            .into_owned()
            .unwrap();
        let mut output = SexpMut::try_from_checked(output).unwrap();
        for (i, value) in data.iter().enumerate() {
            output.try_set_real_elt(i as R_xlen_t, *value).unwrap();
        }
        output.freeze()
    }
    fn integer(factory: &SessionNodeFactory<'_>, data: &[i32]) -> Sexp<'static> {
        let output = factory
            .allocate(|arena| {
                arena
                    .alloc_vector_sexp(SEXPTYPE::INTSXP, data.len() as R_xlen_t)
                    .map(|x| x.as_raw())
            })
            .unwrap()
            .into_owned()
            .unwrap();
        let mut output = SexpMut::try_from_checked(output).unwrap();
        for (i, value) in data.iter().enumerate() {
            output.try_set_integer_elt(i as R_xlen_t, *value).unwrap();
        }
        output.freeze()
    }
    fn symbol(session: &RSession, name: &str) -> Sexp<'static> {
        let text = std::ffi::CString::new(name).unwrap();
        unsafe {
            session
                .owner_token()
                .unwrap()
                .sexp(crate::sexp::symbol::Rf_install(text.as_ptr()))
                .unwrap()
                .into_owned()
                .unwrap()
        }
    }
    fn request(
        factory: &SessionNodeFactory<'_>,
        name: &Sexp<'static>,
        values: &[(Sexp<'static>, Sexp<'static>)],
    ) -> Sexp<'static> {
        let mut result = factory.nil().into_owned().unwrap();
        for (value, tag) in values.iter().rev() {
            result = factory
                .pairlist_cell(value, &result, tag)
                .unwrap()
                .into_owned()
                .unwrap();
        }
        factory
            .pairlist_cell(name, &result, &factory.nil())
            .unwrap()
            .into_owned()
            .unwrap()
    }
    fn invoke_request(
        session: &RSession,
        arguments: &Sexp<'static>,
        interface: BufferInterface,
    ) -> SexpResult<Sexp<'static>> {
        let name = if interface == BufferInterface::C {
            ".C"
        } else {
            ".Fortran"
        };
        let op = unsafe {
            session
                .owner_token()
                .unwrap()
                .sexp(crate::eval::primitive::make_primitive_binding(
                    name,
                    SEXPTYPE::BUILTINSXP,
                ))
                .unwrap()
                .into_owned()
                .unwrap()
        };
        let nil = session
            .owner_token()
            .unwrap()
            .node_factory()
            .nil()
            .into_owned()
            .unwrap();
        unsafe {
            invoke(
                nil.as_raw(),
                op.as_raw(),
                arguments.as_raw(),
                nil.as_raw(),
                interface,
            )
        }
    }
    fn kmeans_arguments(factory: &SessionNodeFactory<'_>) -> Vec<Sexp<'static>> {
        vec![
            real(factory, &[1., 2., 8., 9.]),
            integer(factory, &[4]),
            integer(factory, &[1]),
            real(factory, &[1., 9.]),
            integer(factory, &[2]),
            integer(factory, &[0; 4]),
            integer(factory, &[10]),
            integer(factory, &[0; 2]),
            real(factory, &[0.; 2]),
        ]
    }

    #[test]
    fn owned_buffer_handlers_reject_interface_count_package_type_and_shapes_before_kernel() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let factory = session.owner_token().unwrap().node_factory();
            let nil = factory.nil().into_owned().unwrap();
            let name = factory
                .strings(&["C_kmeans_Lloyd"])
                .unwrap()
                .into_owned()
                .unwrap();
            let values = kmeans_arguments(&factory);
            let before = buffers::invocation_count();
            let make = |values: Vec<Sexp<'static>>| {
                request(
                    &factory,
                    &name,
                    &values
                        .into_iter()
                        .map(|v| (v, nil.clone()))
                        .collect::<Vec<_>>(),
                )
            };
            let wrong_interface = make(values.clone());
            assert!(
                invoke_request(&session, &wrong_interface, BufferInterface::Fortran)
                    .unwrap_err()
                    .to_string()
                    .contains("routine called through")
            );
            for count in [8, 10] {
                let mut changed = values.clone();
                changed.resize(count, nil.clone());
                assert!(
                    invoke_request(&session, &make(changed), BufferInterface::C)
                        .unwrap_err()
                        .to_string()
                        .contains("incorrect number")
                );
            }
            let mut wrong_package: Vec<_> =
                values.iter().cloned().map(|v| (v, nil.clone())).collect();
            wrong_package.insert(
                3,
                (
                    factory.strings(&["tools"]).unwrap().into_owned().unwrap(),
                    symbol(&session, "PACKAGE"),
                ),
            );
            assert!(
                invoke_request(
                    &session,
                    &request(&factory, &name, &wrong_package),
                    BufferInterface::C
                )
                .unwrap_err()
                .to_string()
                .contains("requested PACKAGE")
            );
            let mut wrong = values.clone();
            wrong[1] = real(&factory, &[4.]);
            assert!(
                invoke_request(&session, &make(wrong), BufferInterface::C)
                    .unwrap_err()
                    .to_string()
                    .contains("type")
            );
            for (index, replacement) in [
                (0, real(&factory, &[1.])),
                (1, integer(&factory, &[-1])),
                (2, integer(&factory, &[i32::MAX])),
                (6, integer(&factory, &[0])),
            ] {
                let mut wrong = values.clone();
                wrong[index] = replacement;
                assert!(invoke_request(&session, &make(wrong), BufferInterface::C).is_err());
            }
            assert_eq!(
                buffers::invocation_count(),
                before,
                "every invalid request is rejected before actual leaf invocation"
            );
        });
    }

    #[test]
    fn owned_buffer_kmeans_preserves_aliases_attributes_s4_and_excludes_controls_anywhere() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let factory = session.owner_token().unwrap().node_factory();
            let nil = factory.nil().into_owned().unwrap();
            let name = factory
                .strings(&["C_kmeans_Lloyd"])
                .unwrap()
                .into_owned()
                .unwrap();
            let values = kmeans_arguments(&factory);
            let alias = values[3].clone();
            let marker = factory.strings(&["kept"]).unwrap().into_owned().unwrap();
            let attr = factory
                .pairlist_cell(&marker, &nil, &symbol(&session, "marker"))
                .unwrap()
                .into_owned()
                .unwrap();
            let node = values[0].allocation().unwrap();
            let heap = node.heap_identity();
            let mut core = heap.node_snapshot(node).unwrap();
            core.attrib = factory.link(&attr).unwrap();
            core.sxpinfo.set_obj(true);
            core.sxpinfo.set_gp(core.sxpinfo.gp() | (1 << 4));
            heap.replace_node(node, core).unwrap();
            let mut payload: Vec<_> = values.iter().cloned().map(|v| (v, nil.clone())).collect();
            payload[3].1 = symbol(&session, "centers");
            for (index, tag, value) in [
                (0, "NAOK", factory.nil().into_owned().unwrap()),
                (
                    4,
                    "PACKAGE",
                    factory.strings(&["stats"]).unwrap().into_owned().unwrap(),
                ),
                (
                    7,
                    "DUP",
                    factory.strings(&["ignored"]).unwrap().into_owned().unwrap(),
                ),
                (
                    12,
                    "ENCODING",
                    factory.strings(&["UTF-8"]).unwrap().into_owned().unwrap(),
                ),
            ] {
                let value = if tag == "NAOK" {
                    integer(&factory, &[0])
                } else {
                    value
                };
                payload.insert(index, (value, symbol(&session, tag)));
            }
            let args = request(&factory, &name, &payload);
            let result = invoke_request(&session, &args, BufferInterface::C).unwrap();
            assert_eq!(result.len(), 9);
            let centers = result.try_vector_elt(3).unwrap();
            assert_eq!(centers.try_real_elt(0).unwrap(), 1.5);
            assert_eq!(centers.try_real_elt(1).unwrap(), 8.5);
            assert_eq!(alias.try_real_elt(0).unwrap(), 1.);
            assert_eq!(alias.try_real_elt(1).unwrap(), 9.);
            assert_ne!(centers.as_raw(), alias.as_raw());
            assert_eq!(
                result
                    .try_vector_elt(5)
                    .unwrap()
                    .try_integer_elt(2)
                    .unwrap(),
                2
            );
            let x = result.try_vector_elt(0).unwrap();
            assert_eq!(x.typeof_(), SEXPTYPE::REALSXP);
            assert_ne!(
                x.header().sxpinfo.gp() & (1 << 4),
                0,
                "GNU S4 identity flag must survive numeric-vector publication"
            );
            assert!(x.header().sxpinfo.obj());
            assert_eq!(x.try_attrib().unwrap().as_raw(), attr.as_raw());
            let names = result.try_attrib().unwrap().try_car().unwrap();
            assert_eq!(
                names.try_string_elt(3).unwrap().try_as_string().unwrap(),
                "centers"
            );
            // Source controls and full original pairlist remain intact.
            assert_eq!(
                args.try_cdr()
                    .unwrap()
                    .try_tag()
                    .unwrap()
                    .try_printname()
                    .unwrap()
                    .try_as_string()
                    .unwrap(),
                "NAOK"
            );
        });
    }

    #[test]
    fn owned_buffer_fortran_bvalus_matches_pinned_neighbor_and_base_dtrco_remains_checked() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let factory = session.owner_token().unwrap().node_factory();
            let nil = factory.nil().into_owned().unwrap();
            let name = factory
                .strings(&["C_bvalus"])
                .unwrap()
                .into_owned()
                .unwrap();
            let data = vec![
                integer(&factory, &[3]),
                real(&factory, &[0., 0., 0., 0., 1., 1., 1., 1.]),
                real(&factory, &[0., 1., 2., 3.]),
                integer(&factory, &[4]),
                real(&factory, &[0., 0.5, 1.]),
                real(&factory, &[0.; 3]),
                integer(&factory, &[0]),
            ];
            let payload: Vec<_> = data.into_iter().map(|v| (v, nil.clone())).collect();
            let result = invoke_request(
                &session,
                &request(&factory, &name, &payload),
                BufferInterface::Fortran,
            )
            .unwrap();
            let values = result.try_vector_elt(5).unwrap();
            for (i, expected) in [0., 1.5, 3.].iter().enumerate() {
                assert_eq!(values.try_real_elt(i as R_xlen_t).unwrap(), *expected);
            }
            let name = factory.strings(&["dtrco"]).unwrap().into_owned().unwrap();
            let data = vec![
                real(&factory, &[2., 0., 0., 4.]),
                integer(&factory, &[2]),
                integer(&factory, &[2]),
                real(&factory, &[0.]),
                real(&factory, &[0.; 2]),
                integer(&factory, &[1]),
            ];
            let mut payload: Vec<_> = data.into_iter().map(|v| (v, nil.clone())).collect();
            payload.insert(
                2,
                (
                    factory.strings(&["base"]).unwrap().into_owned().unwrap(),
                    symbol(&session, "PACKAGE"),
                ),
            );
            let result = invoke_request(
                &session,
                &request(&factory, &name, &payload),
                BufferInterface::Fortran,
            )
            .unwrap();
            assert!(result.try_vector_elt(3).unwrap().try_real_elt(0).unwrap() > 0.);
        });
    }

    struct CollectingReal {
        calls: Rc<Cell<usize>>,
        source: Rc<Cell<SEXP>>,
        attribute: Rc<RefCell<Option<crate::sexp::heap::CheckedNode>>>,
        session: std::rc::Weak<RefCell<Option<RSession>>>,
        close: bool,
    }
    impl crate::sexp::altrep::AltrepClass for CollectingReal {
        fn vector_type(&self) -> SEXPTYPE {
            SEXPTYPE::REALSXP
        }
        fn length(&self, _: &crate::sexp::altrep::AltrepContext<'_>) -> SexpResult<R_xlen_t> {
            Ok(4)
        }
        fn element<'s>(
            &self,
            context: &crate::sexp::altrep::AltrepContext<'s>,
            index: R_xlen_t,
        ) -> SexpResult<crate::sexp::altrep::AltrepElement<'s>> {
            if self.calls.get() == 0 {
                self.calls.set(1);
                let source = self.source.get();
                // No payload loan spans this callback. The original owning
                // operand snapshots must survive actual source graph removal.
                unsafe {
                    crate::sexp::accessors::SETCDR(source, crate::sexp::globals::R_NilValue());
                }
                context.gc()?;
                assert!(self.attribute.borrow().as_ref().unwrap().is_live());
                if self.close {
                    self.session
                        .upgrade()
                        .unwrap()
                        .borrow_mut()
                        .as_mut()
                        .unwrap()
                        .close();
                }
            }
            Ok(crate::sexp::altrep::AltrepElement::Real(
                [1., 2., 8., 9.][index as usize],
            ))
        }
    }

    #[test]
    fn owned_buffer_altrep_detaches_source_collects_and_denies_closed_publication() {
        use crate::sexp::altrep::AltrepBuilder;
        for close in [false, true] {
            let sessions = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
            let calls = Rc::new(Cell::new(0));
            let source = Rc::new(Cell::new(std::ptr::null_mut()));
            let attribute = Rc::new(RefCell::new(None));
            let (args, op, nil) = {
                let borrow = sessions.borrow();
                let session = borrow.as_ref().unwrap();
                let factory = session.owner_token().unwrap().node_factory();
                let nil = factory.nil().into_owned().unwrap();
                let class = session
                    .register_altrep_class(
                        "buffer_collecting_real",
                        CollectingReal {
                            calls: calls.clone(),
                            source: source.clone(),
                            attribute: attribute.clone(),
                            session: Rc::downgrade(&sessions),
                            close,
                        },
                    )
                    .unwrap()
                    .into_owned()
                    .unwrap();
                let value = AltrepBuilder::new(class)
                    .build()
                    .unwrap()
                    .into_owned()
                    .unwrap();
                let mut values = kmeans_arguments(&factory);
                values[0] = value;
                let marker = factory
                    .pairlist_cell(&integer(&factory, &[777]), &nil, &symbol(session, "marker"))
                    .unwrap()
                    .into_owned()
                    .unwrap();
                *attribute.borrow_mut() = Some(marker.allocation().unwrap().clone());
                let node = values[3].allocation().unwrap();
                let heap = node.heap_identity();
                let mut core = heap.node_snapshot(node).unwrap();
                core.attrib = factory.link(&marker).unwrap();
                heap.replace_node(node, core).unwrap();
                let name = factory
                    .strings(&["C_kmeans_Lloyd"])
                    .unwrap()
                    .into_owned()
                    .unwrap();
                let args = request(
                    &factory,
                    &name,
                    &values
                        .into_iter()
                        .map(|v| (v, nil.clone()))
                        .collect::<Vec<_>>(),
                );
                source.set(args.as_raw());
                let op = unsafe {
                    session
                        .owner_token()
                        .unwrap()
                        .sexp(crate::eval::primitive::make_primitive_binding(
                            ".C",
                            SEXPTYPE::BUILTINSXP,
                        ))
                        .unwrap()
                        .into_owned()
                        .unwrap()
                };
                (args, op, nil)
            };
            let notifications = Rc::new(Cell::new(0));
            let observed = notifications.clone();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                observed.set(observed.get() + 1)
            }));
            let before = buffers::invocation_count();
            let result = unsafe {
                invoke(
                    nil.as_raw(),
                    op.as_raw(),
                    args.as_raw(),
                    nil.as_raw(),
                    BufferInterface::C,
                )
            };
            assert_eq!(calls.get(), 1);
            assert!(
                notifications.get() > 0,
                "real provider collection must execute"
            );
            if close {
                assert!(result.is_err());
                assert_eq!(buffers::invocation_count(), before);
            } else {
                let result = result.unwrap();
                assert_eq!(
                    result.try_vector_elt(3).unwrap().try_real_elt(0).unwrap(),
                    1.5
                );
                assert_eq!(buffers::invocation_count(), before + 1);
                assert!(attribute.borrow().as_ref().unwrap().is_live());
            }
        }
    }
    struct CollectingPackage {
        name_container: Rc<Cell<SEXP>>,
        child: Rc<RefCell<Option<crate::sexp::heap::CheckedNode>>>,
        calls: Rc<Cell<usize>>,
        session: std::rc::Weak<RefCell<Option<RSession>>>,
        close: bool,
    }
    impl crate::sexp::altrep::AltrepClass for CollectingPackage {
        fn vector_type(&self) -> SEXPTYPE {
            SEXPTYPE::STRSXP
        }
        fn length(&self, _: &crate::sexp::altrep::AltrepContext<'_>) -> SexpResult<R_xlen_t> {
            Ok(1)
        }
        fn element<'s>(
            &self,
            context: &crate::sexp::altrep::AltrepContext<'s>,
            _: R_xlen_t,
        ) -> SexpResult<crate::sexp::altrep::AltrepElement<'s>> {
            self.calls.set(self.calls.get() + 1);
            let text = context.string("stats")?;
            unsafe {
                crate::sexp::accessors::SET_VECTOR_ELT(
                    self.name_container.get(),
                    0,
                    crate::sexp::globals::R_NilValue(),
                );
            }
            context.gc()?;
            assert!(
                self.child.borrow().as_ref().unwrap().is_live(),
                "original lookup child is held only by admission snapshot after detachment"
            );
            if self.close {
                self.session
                    .upgrade()
                    .unwrap()
                    .borrow_mut()
                    .as_mut()
                    .unwrap()
                    .close();
            }
            Ok(crate::sexp::altrep::AltrepElement::String(text))
        }
    }

    #[test]
    fn owned_buffer_package_provider_retains_original_symbol_name_and_rejects_revocation() {
        use crate::sexp::altrep::AltrepBuilder;
        for close in [false, true] {
            let sessions = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
            let container = Rc::new(Cell::new(std::ptr::null_mut()));
            let child = Rc::new(RefCell::new(None));
            let calls = Rc::new(Cell::new(0));
            let (args, op, nil) = {
                let borrow = sessions.borrow();
                let session = borrow.as_ref().unwrap();
                let factory = session.owner_token().unwrap().node_factory();
                let nil = factory.nil().into_owned().unwrap();
                let class = session
                    .register_altrep_class(
                        "buffer_collecting_package",
                        CollectingPackage {
                            name_container: container.clone(),
                            child: child.clone(),
                            calls: calls.clone(),
                            session: Rc::downgrade(&sessions),
                            close,
                        },
                    )
                    .unwrap()
                    .into_owned()
                    .unwrap();
                let package = AltrepBuilder::new(class)
                    .build()
                    .unwrap()
                    .into_owned()
                    .unwrap();
                let original = factory
                    .strings(&["C_kmeans_Lloyd"])
                    .unwrap()
                    .into_owned()
                    .unwrap();
                *child.borrow_mut() = Some(original.allocation().unwrap().clone());
                let symbol_info = factory
                    .allocate(|arena| {
                        arena
                            .alloc_vector_sexp(SEXPTYPE::VECSXP, 1)
                            .map(|x| x.as_raw())
                    })
                    .unwrap()
                    .into_owned()
                    .unwrap();
                let mut mutation = SexpMut::try_from_checked(symbol_info).unwrap();
                mutation.try_set_vector_elt(0, original).unwrap();
                let symbol_info = mutation.freeze();
                container.set(symbol_info.as_raw());
                let mut payload: Vec<_> = kmeans_arguments(&factory)
                    .into_iter()
                    .map(|x| (x, nil.clone()))
                    .collect();
                payload.insert(2, (package, symbol(session, "PACKAGE")));
                let args = request(&factory, &symbol_info, &payload);
                let op = unsafe {
                    session
                        .owner_token()
                        .unwrap()
                        .sexp(crate::eval::primitive::make_primitive_binding(
                            ".C",
                            SEXPTYPE::BUILTINSXP,
                        ))
                        .unwrap()
                        .into_owned()
                        .unwrap()
                };
                (args, op, nil)
            };
            let before = buffers::invocation_count();
            let result = unsafe {
                invoke(
                    nil.as_raw(),
                    op.as_raw(),
                    args.as_raw(),
                    nil.as_raw(),
                    BufferInterface::C,
                )
            };
            assert_eq!(calls.get(), 1);
            if close {
                assert!(result.is_err());
                assert_eq!(buffers::invocation_count(), before);
            } else {
                assert_eq!(
                    result
                        .unwrap()
                        .try_vector_elt(3)
                        .unwrap()
                        .try_real_elt(0)
                        .unwrap(),
                    1.5
                );
                assert_eq!(buffers::invocation_count(), before + 1);
            }
        }
    }
    #[test]
    fn owned_buffer_result_promoted_by_callback_remembers_new_young_names() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let factory = session.owner_token().unwrap().node_factory();
            let nil = factory.nil().into_owned().unwrap();
            let name = factory
                .strings(&["C_kmeans_Lloyd"])
                .unwrap()
                .into_owned()
                .unwrap();
            let mut payload: Vec<_> = kmeans_arguments(&factory)
                .into_iter()
                .map(|v| (v, nil.clone()))
                .collect();
            payload[3].1 = symbol(&session, "centers");
            let args = request(&factory, &name, &payload);
            // Capture allocation generations, not addresses: a callback may
            // collect unrelated baseline objects and reuse their slots.
            let existing = session.with_active_in(|instance| unsafe {
                let arena = &(*instance).arena;
                arena
                    .nodes()
                    .filter_map(|p| arena.node_token(p)?.link())
                    .collect::<std::collections::HashSet<_>>()
            });
            let owner = session.owner_token().unwrap().weak_owner().unwrap();
            let promoted = Rc::new(RefCell::new(None));
            let observed = promoted.clone();
            let collections = Rc::new(Cell::new(0));
            let notifications = collections.clone();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                notifications.set(notifications.get() + 1);
                if observed.borrow().is_some() {
                    return;
                }
                let pin = owner.pin().unwrap();
                let instance = pin.as_ptr();
                let candidate = unsafe {
                    let arena = &(*instance).arena;
                    arena.nodes().find_map(|p| {
                        let node = arena.node_token(p)?;
                        if existing.contains(&node.link()?) {
                            return None;
                        }
                        let header = node.heap_identity().node_snapshot(&node)?;
                        match header.data {
                            crate::sexp::ffi::NodeBody::Vector(vector)
                                if header.sxpinfo.type_of() == SEXPTYPE::VECSXP
                                    && vector.length == 9 =>
                            {
                                Some(node)
                            }
                            _ => None,
                        }
                    })
                };
                if let Some(node) = candidate {
                    // Promote earlier numerical output nodes too. Otherwise
                    // vector-slot writes could independently remember result,
                    // making a missing names-attribute barrier unobservable.
                    let outputs: Vec<_> = unsafe {
                        let arena = &(*instance).arena;
                        arena
                            .nodes()
                            .filter_map(|p| {
                                let child = arena.node_token(p)?;
                                if existing.contains(&child.link()?) {
                                    return None;
                                }
                                let header = child.heap_identity().node_snapshot(&child)?;
                                matches!(
                                    header.sxpinfo.type_of(),
                                    SEXPTYPE::REALSXP | SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP
                                )
                                .then_some(child)
                            })
                            .collect()
                    };
                    for child in outputs {
                        let heap = child.heap_identity();
                        let mut header = heap.node_snapshot(&child).unwrap();
                        header
                            .sxpinfo
                            .set_gcgen(crate::sexp::gengc::Generation::Old as u8);
                        heap.replace_node(&child, header).unwrap();
                    }
                    let heap = node.heap_identity();
                    let mut header = heap.node_snapshot(&node).unwrap();
                    header
                        .sxpinfo
                        .set_gcgen(crate::sexp::gengc::Generation::Old as u8);
                    heap.replace_node(&node, header).unwrap();
                    // Later names cells remain young: no further allocation
                    // collection occurs after the actual result is promoted.
                    unsafe {
                        (*instance).memory_state.gc_force_gap = 0;
                    }
                    *observed.borrow_mut() = Some(node);
                }
            }));
            session.with_active_in(|instance| unsafe {
                (*instance).memory_state.gc_force_gap = 1;
                (*instance).memory_state.gc_force_wait = 1;
            });
            let result = invoke_request(&session, &args, BufferInterface::C).unwrap();
            assert!(
                collections.get() > 0,
                "real allocation collection callbacks must execute"
            );
            assert_eq!(
                promoted.borrow().as_ref().unwrap().link(),
                result.allocation().unwrap().link()
            );
            assert_eq!(
                result.header().sxpinfo.gcgen(),
                crate::sexp::gengc::Generation::Old as u8
            );
            let attributes = result.try_attrib().unwrap();
            assert_eq!(
                attributes.header().sxpinfo.gcgen(),
                crate::sexp::gengc::Generation::Young as u8
            );
            session.with_active_in(|instance| unsafe {
                assert!(
                    (*instance)
                        .gc_state
                        .remembered_set
                        .iter()
                        .any(|p| p == result.as_raw()),
                    "production attribute publication must record the old result's young names edge"
                );
            });
            crate::sexp::gengc::minor_gc();
            assert_eq!(
                result
                    .try_attrib()
                    .unwrap()
                    .try_car()
                    .unwrap()
                    .try_string_elt(3)
                    .unwrap()
                    .try_as_string()
                    .unwrap(),
                "centers"
            );
        });
    }
}
