#![allow(unsafe_code)]
//! The physical representation of an ALTREP instance. Slot numbers, traced
//! edges and header publication are private to this module; callers use roots.
use super::registry::{CachePolicy, VectorKind};
use super::*;

const TAG: &std::ffi::CStr = c".InternalAltrep";
const SLOT_COUNT: i64 = 5;
#[derive(Clone, Copy)]
enum Slot {
    Descriptor,
    Data1,
    Data2,
    DenseCache,
    NativeChildren,
}
impl Slot {
    fn index(self) -> i64 {
        match self {
            Self::Descriptor => 0,
            Self::Data1 => 1,
            Self::Data2 => 2,
            Self::DenseCache => 3,
            Self::NativeChildren => 4,
        }
    }
}

/// A traced metadata vector, checked once before named field access. Loading
/// also works for non-owning probes and does not borrow the interpreter owner.
#[derive(Clone)]
pub(super) struct Metadata<'s> {
    slots: Sexp<'s>,
}
impl<'s> Metadata<'s> {
    pub(super) fn load(object: &Sexp<'s>) -> Option<Self> {
        if !object.header().sxpinfo.alt() {
            return None;
        }
        let cell = object.attrib()?;
        if cell.typeof_() != SEXPTYPE::LISTSXP {
            return None;
        }
        let tag = cell.tag()?;
        let super::super::object::NodeBody::Symbol(symbol) = tag.header().body else {
            return None;
        };
        if !tag
            .copied_header_link(symbol.pname)?
            .char_eq(TAG.to_bytes())
        {
            return None;
        }
        let slots = cell.car()?;
        (slots.typeof_() == SEXPTYPE::VECSXP && slots.len() == SLOT_COUNT).then_some(Self { slots })
    }
    fn get(&self, slot: Slot) -> SexpResult<Sexp<'s>> {
        self.slots.try_vector_elt(slot.index())
    }
    fn set(&self, slot: Slot, value: Sexp<'s>) -> SexpResult<()> {
        SexpMut::try_from_checked(self.slots.clone())?.try_set_vector_elt(slot.index(), value)
    }
    pub(super) fn descriptor(&self) -> SexpResult<Sexp<'s>> {
        self.get(Slot::Descriptor)
    }
    pub(super) fn data1(&self) -> SexpResult<Sexp<'s>> {
        self.get(Slot::Data1)
    }
    pub(super) fn data2(&self) -> SexpResult<Sexp<'s>> {
        self.get(Slot::Data2)
    }
    pub(super) fn set_data1(&self, value: Sexp<'s>) -> SexpResult<()> {
        self.set(Slot::Data1, value)
    }
    pub(super) fn set_data2(&self, value: Sexp<'s>) -> SexpResult<()> {
        self.set(Slot::Data2, value)
    }
    #[cfg(test)]
    pub(super) fn native_children(&self) -> SexpResult<Sexp<'s>> {
        self.get(Slot::NativeChildren)
    }

    /// Native pointer readers need the parent to retain the returned child.
    /// Keep a sparse traced cache rather than expanding a huge pointer vector.
    pub(super) fn retain_child(
        &self,
        owner: OwnerToken<'s>,
        index: i64,
        child: Sexp<'s>,
    ) -> SexpResult<()> {
        let child = owner.sexp(child.as_raw())?;
        let mut cell = self.get(Slot::NativeChildren)?;
        let mut seen = std::collections::HashSet::new();
        while cell.typeof_() != SEXPTYPE::NILSXP {
            seen.try_reserve(1)
                .map_err(|_| failure("native element cache walk"))?;
            if !seen.insert(cell.clone().as_raw() as usize) {
                return Err(failure("cyclic native element cache"));
            }
            let entry = cell.try_car()?;
            if entry.try_vector_elt(0)?.try_real_elt(0)? == index as f64 {
                return SexpMut::try_from_checked(entry)?.try_set_vector_elt(1, child);
            }
            cell = cell.try_cdr()?;
        }
        let mut key = SexpMut::try_from_checked(allocate(owner, SEXPTYPE::REALSXP, 1)?)?;
        key.try_set_real_elt(0, index as f64)?;
        let mut entry = SexpMut::try_from_checked(allocate(owner, SEXPTYPE::VECSXP, 2)?)?;
        entry.try_set_vector_elt(0, key.freeze())?;
        entry.try_set_vector_elt(1, child)?;
        let tail = self.get(Slot::NativeChildren)?;
        let cell = cons(owner, entry.freeze(), tail, Sexp::nil())?;
        self.set(Slot::NativeChildren, cell)
    }
}

pub(super) fn owner<'s>(object: &Sexp<'s>) -> SexpResult<OwnerToken<'s>> {
    let pin = object.pin_runtime()?;
    let pointer = pin
        .as_ref()
        .map(|owner| owner.as_ptr())
        .or_else(|| object.session_owner_ptr.map(|owner| owner.as_ptr()))
        .ok_or(SexpError::UncheckedMutation)?;
    // The enclosing operation retains its original pin through callbacks.
    Ok(unsafe { OwnerToken::from_raw(pointer) })
}
pub(super) fn activate<T>(owner: OwnerToken<'_>, callback: impl FnOnce() -> T) -> T {
    // SAFETY: only a lifetime-bound checked owner capability enters here.
    unsafe { with_instance_active(owner.as_ptr(), callback) }
}
pub(super) fn allocate<'s>(
    owner: OwnerToken<'s>,
    kind: SEXPTYPE,
    length: R_xlen_t,
) -> SexpResult<Sexp<'s>> {
    VectorKind::from_sexp(kind)?;
    usize::try_from(length).map_err(|_| failure("invalid vector length"))?;
    allocate_rooted(owner, |arena| {
        arena
            .alloc_vector_sexp(kind, length)
            .map(|value| value.as_raw())
    })
    .map_err(|_| failure("ALTREP vector"))
}
pub(super) fn string<'s>(owner: OwnerToken<'s>, text: &str) -> SexpResult<Sexp<'s>> {
    allocate_rooted(owner, |arena| {
        arena
            .alloc_charsxp_sexp(text.as_bytes())
            .map(|value| value.as_raw())
    })
}
pub(super) fn intern<'s>(owner: OwnerToken<'s>, name: &std::ffi::CStr) -> SexpResult<Sexp<'s>> {
    // SAFETY: terminated name stays live; interned symbol belongs to this owner.
    let raw = activate(owner, || unsafe {
        super::super::symbol::Rf_install(name.as_ptr())
    });
    owner.sexp(raw)
}
fn cons<'s>(
    owner: OwnerToken<'s>,
    car: Sexp<'s>,
    cdr: Sexp<'s>,
    tag: Sexp<'s>,
) -> SexpResult<Sexp<'s>> {
    let car = owner.sexp(car.as_raw())?;
    let cdr = owner.sexp(cdr.as_raw())?;
    let tag = owner.sexp(tag.as_raw())?;
    // SAFETY: session-validated children stay rooted across this local arena
    // allocation. Tags may be interned symbols outside the arena itself.
    allocate_rooted(owner, |arena| {
        Some(unsafe {
            arena.cons(
                car.clone().as_raw(),
                cdr.clone().as_raw(),
                tag.clone().as_raw(),
            )
        })
    })
}

/// Install the automatic value lease inside the allocation lend, before
/// deferred collection and provider reentry can observe the fresh graph.
fn allocate_rooted<'s>(
    owner: OwnerToken<'s>,
    allocation: impl FnOnce(&mut super::super::memory::RArena) -> Option<SEXP>,
) -> SexpResult<Sexp<'s>> {
    let factory = super::super::object::SessionNodeFactory::new(owner);
    activate(owner, || factory.allocate(allocation))
}

/// A callback may mutate provider data, but cannot replace the declaration
/// whose provider and representation were sampled for this operation.
struct Declaration<'s> {
    descriptor: Sexp<'s>,
    metadata_payload: super::super::payload::PayloadLink,
    kind: SEXPTYPE,
    length: R_xlen_t,
    payload: super::super::payload::PayloadLink,
}

/// Owns the roots necessary for construction or final payload publication.
/// Provider callbacks receive handles, never this physical storage capability.
pub(super) struct InstanceStorage<'s> {
    owner: OwnerToken<'s>,
    object: Sexp<'s>,
    metadata: Metadata<'s>,
    declaration: Declaration<'s>,
}
impl<'s> InstanceStorage<'s> {
    pub(super) fn create(
        class: &AltrepClassHandle<'s>,
        kind: VectorKind,
        data1: Sexp<'s>,
        data2: Sexp<'s>,
    ) -> SexpResult<PendingInstance<'s>> {
        let owner = class.owner;
        let data1 = owner.sexp(data1.as_raw())?;
        let data2 = owner.sexp(data2.as_raw())?;
        let object = allocate(owner, kind.sexp_type(), 0)?;
        let mut slots = SexpMut::try_from_checked(allocate(owner, SEXPTYPE::VECSXP, SLOT_COUNT)?)?;
        slots.try_set_vector_elt(Slot::Descriptor.index(), class.descriptor.clone())?;
        slots.try_set_vector_elt(Slot::Data1.index(), data1)?;
        slots.try_set_vector_elt(Slot::Data2.index(), data2)?;
        let metadata = Metadata {
            slots: slots.freeze(),
        };
        let tag = intern(owner, TAG)?;
        let cell = cons(owner, metadata.slots.clone(), Sexp::nil(), tag)?;
        link_attributes(owner, &object, &cell)?;
        // The pending header has traced metadata before Length callbacks run.
        update_header(&object, |header| header.sxpinfo.set_alt(true))?;
        let mut storage = Self::load(&object)?;
        storage.declaration.descriptor = class.descriptor.clone();
        storage.validate()?;
        Ok(PendingInstance { storage })
    }
    pub(super) fn load(object: &Sexp<'s>) -> SexpResult<Self> {
        let metadata = Metadata::load(object).ok_or(failure("invalid ALTREP metadata"))?;
        let declaration = Declaration {
            descriptor: metadata.descriptor()?,
            metadata_payload: metadata.slots.header().payload,
            kind: object.typeof_(),
            length: object.len(),
            payload: object.header().payload,
        };
        Ok(Self {
            owner: owner(object)?,
            object: object.clone(),
            metadata,
            declaration,
        })
    }
    pub(super) fn validate(&self) -> SexpResult<()> {
        let current = Metadata::load(&self.object)
            .ok_or(failure("ALTREP declaration changed during callback"))?;
        let heap = self.object.allocation()?.heap_identity();
        if current.slots.link_in(&heap)? != self.metadata.slots.link_in(&heap)?
            || current.slots.header().payload != self.declaration.metadata_payload
            || current.descriptor()?.link_in(&heap)?
                != self.declaration.descriptor.link_in(&heap)?
            || self.object.typeof_() != self.declaration.kind
            || self.object.len() != self.declaration.length
            || self.object.header().payload != self.declaration.payload
        {
            return Err(failure("ALTREP declaration changed during callback"));
        }
        Ok(())
    }
    pub(super) fn publish_dense(&self, output: Sexp<'s>, policy: CachePolicy) -> SexpResult<()> {
        self.validate()?;
        let output = self.owner.sexp(output.as_raw())?;
        if output.typeof_() != self.declaration.kind || output.len() != self.declaration.length {
            return Err(failure("ALTREP cache shape mismatch"));
        }
        if super::super::memory::is_arena_lent(self.owner.as_ptr()) {
            return Err(failure("release the arena lend before materialization"));
        }
        let parent = self.object.allocation()?;
        let heap = parent.heap_identity();
        let source = output.allocation()?;
        let lease = heap
            .payload_lease(source)
            .ok_or(failure("ALTREP cache payload"))?;
        let slots = self.metadata.slots.allocation()?;
        let cells = heap
            .reference_payload_lease(slots)
            .ok_or(failure("ALTREP cache slots"))?;
        let output_link = output.link_in(&heap)?;
        let changes = [
            (Slot::DenseCache.index() as usize, output_link),
            (Slot::Data2.index() as usize, output_link),
        ];
        let changes = &changes[..if matches!(policy, CachePolicy::Data2) {
            2
        } else {
            1
        }];
        // Check writability and every bound without changing any cache cell.
        cells
            .replace_sparse(&[])
            .ok_or(failure("ALTREP immutable cache slots"))?;
        if changes
            .iter()
            .any(|(index, _)| cells.element(*index).is_none())
        {
            return Err(failure("ALTREP cache slot bounds"));
        }
        barrier(self.owner, &self.object, &output)?;
        barrier(self.owner, &self.metadata.slots, &output)?;
        self.validate()?;
        // This exact heap publication performs its fallible storage admission
        // before changing the header. It runs no R callback or deferred lend.
        heap.publish_payload(parent, self.declaration.payload, &lease)
            .ok_or(failure("ALTREP cache payload publication"))?;
        let mut header = heap
            .node_snapshot(parent)
            .expect("published ALTREP parent stays live");
        header.data.vector_mut().truelength = self.declaration.length;
        heap.replace_node(parent, header)
            .expect("published ALTREP payload retains its valid shape");
        // No callback or lease mutation occurs between preflight and commit.
        cells
            .replace_sparse(changes)
            .expect("preflighted ALTREP cache cells stay writable");
        Ok(())
    }
}
/// Construction and publication are separate capabilities. Only a pending
/// instance can set its logical length, and finishing consumes that capability.
pub(super) struct PendingInstance<'s> {
    storage: InstanceStorage<'s>,
}
impl<'s> PendingInstance<'s> {
    pub(super) fn context(&self) -> AltrepContext<'s> {
        AltrepContext {
            pin: self
                .storage
                .object
                .pin_runtime()
                .expect("live ALTREP construction owner"),
            owner: self.storage.owner,
            object: self.storage.object.clone(),
            metadata: self.storage.metadata.clone(),
        }
    }
    pub(super) fn finish(self, length: R_xlen_t) -> SexpResult<Sexp<'s>> {
        if length < 0 || length > (1_i64 << 52) {
            return Err(failure("invalid ALTREP length"));
        }
        self.storage.validate()?;
        let object = self.storage.object;
        if object.len() != 0
            || !object.header().payload.is_empty()
            || Metadata::load(&object).is_none()
        {
            return Err(failure(
                "ALTREP construction changed its pending representation",
            ));
        }
        // This consuming capability publishes the trusted lazy length.
        update_header(&object, |header| header.data.vector_mut().length = length)?;
        Ok(object)
    }
}

#[forbid(unsafe_code)]
fn update_header(
    object: &Sexp<'_>,
    edit: impl FnOnce(&mut super::super::ffi::SexprecCore),
) -> SexpResult<()> {
    let (_, parent) = super::super::memory::checked_projection(object.as_raw())
        .ok_or(failure("ALTREP header parent"))?;
    let heap = parent.heap_identity();
    let mut header = heap
        .node_snapshot(&parent)
        .ok_or(failure("ALTREP header"))?;
    edit(&mut header);
    heap.replace_node(&parent, header)
        .ok_or(failure("ALTREP header publication"))
}

fn barrier(owner: OwnerToken<'_>, parent: &Sexp<'_>, child: &Sexp<'_>) -> SexpResult<()> {
    // SAFETY: both rooted nodes are validated in this owner before publication.
    if !unsafe {
        super::super::gengc::write_barrier_in(
            owner.as_ptr(),
            parent.clone().as_raw(),
            child.clone().as_raw(),
        )
    } {
        return Err(failure("ALTREP edge barrier"));
    }
    Ok(())
}
fn link_attributes(
    owner: OwnerToken<'_>,
    object: &Sexp<'_>,
    attributes: &Sexp<'_>,
) -> SexpResult<()> {
    barrier(owner, object, attributes)?;
    let (_, parent) = super::super::memory::checked_projection(object.as_raw())
        .ok_or(failure("ALTREP attribute parent"))?;
    let heap = parent.heap_identity();
    let link = attributes.link_in(&heap)?;
    let mut header = heap
        .node_snapshot(&parent)
        .ok_or(failure("ALTREP attribute header"))?;
    header.attrib = link;
    heap.replace_node(&parent, header)
        .ok_or(failure("ALTREP attribute publication"))
}
pub(super) fn copy_public_attributes(source: &Sexp<'_>, target: &Sexp<'_>) -> SexpResult<()> {
    let owner = owner(target)?;
    let attributes = if Metadata::load(source).is_some() {
        source.attrib().and_then(|cell| cell.cdr())
    } else {
        source.attrib()
    };
    if let Some(attributes) = attributes {
        link_attributes(owner, target, &attributes)?;
        let source = source.header();
        update_header(target, |header| {
            header.sxpinfo.set_obj(source.sxpinfo.obj());
            header.sxpinfo.set_gp(source.sxpinfo.gp());
        })?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "safety_tests.rs"]
mod safety_tests;
