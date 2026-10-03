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
        if !tag.copied_header_link(symbol.pname)?.char_eq(TAG.to_bytes()) {
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
    let pointer = object
        .session_owner_ptr
        .ok_or(SexpError::UncheckedMutation)?;
    // SAFETY: the checked root retains its original session lifetime.
    Ok(unsafe { OwnerToken::from_raw(pointer.as_ptr()) })
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
        arena.alloc_vector_sexp(kind, length).map(|value| value.as_raw())
    })
    .map_err(|_| failure("ALTREP vector"))
}
pub(super) fn string<'s>(owner: OwnerToken<'s>, text: &str) -> SexpResult<Sexp<'s>> {
    allocate_rooted(owner, |arena| {
        arena.alloc_charsxp_sexp(text.as_bytes()).map(|value| value.as_raw())
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

/// Owns the roots necessary for construction or final payload publication.
/// Provider callbacks receive handles, never this physical storage capability.
pub(super) struct InstanceStorage<'s> {
    owner: OwnerToken<'s>,
    object: Sexp<'s>,
    metadata: Metadata<'s>,
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
        // SAFETY: this fresh rooted header has a traced metadata edge and no
        // payload. Native Length can now read data1/data2 during construction.
        unsafe {
            (*object.clone().as_raw()).sxpinfo.set_alt(true);
        }
        Ok(PendingInstance {
            storage: Self {
                owner,
                object,
                metadata,
            },
        })
    }
    pub(super) fn load(object: &Sexp<'s>) -> SexpResult<Self> {
        Ok(Self {
            owner: owner(object)?,
            object: object.clone(),
            metadata: Metadata::load(object).ok_or(failure("invalid ALTREP metadata"))?,
        })
    }
    pub(super) fn publish_dense(&self, output: Sexp<'s>, policy: CachePolicy) -> SexpResult<()> {
        let output = self.owner.sexp(output.as_raw())?;
        if output.typeof_() != self.object.typeof_() || output.len() != self.object.len() {
            return Err(failure("ALTREP cache shape mismatch"));
        }
        if super::super::memory::is_arena_lent(self.owner.as_ptr()) {
            return Err(failure("release the arena lend before materialization"));
        }
        barrier(self.owner, &self.object, &output)?;
        self.metadata.set(Slot::DenseCache, output.clone())?;
        if matches!(policy, CachePolicy::Data2) {
            self.metadata.set_data2(output.clone())?;
        }
        // SAFETY: no callback or payload loan occurs during this short lend.
        activate(self.owner, || unsafe {
            super::super::memory::with_arena(|arena| {
                arena.share_vector_payload(&output, &self.object)
            })
        })?;
        // SAFETY: matching rooted headers now share an arena-owned buffer lease.
        unsafe {
            (*self.object.clone().as_raw()).set_vecsxp_truelength(self.object.len());
        }
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
            owner: self.storage.owner,
            object: self.storage.object.clone(),
            metadata: self.storage.metadata.clone(),
        }
    }
    pub(super) fn finish(self, length: R_xlen_t) -> SexpResult<Sexp<'s>> {
        if length < 0 || length > (1_i64 << 52) {
            return Err(failure("invalid ALTREP length"));
        }
        let object = self.storage.object;
        if object.len() != 0
            || !object.header().payload.is_null()
            || Metadata::load(&object).is_none()
        {
            return Err(failure(
                "ALTREP construction changed its pending representation",
            ));
        }
        // SAFETY: a pending rooted header with traced metadata has no payload
        // yet. This consuming capability is the only logical-length setter.
        unsafe {
            (*object.clone().as_raw()).set_vecsxp_length(length);
        }
        Ok(object)
    }
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
    let mut header = heap.node_snapshot(&parent).ok_or(failure("ALTREP attribute header"))?;
    header.attrib = link;
    heap.replace_node(&parent, header).ok_or(failure("ALTREP attribute publication"))
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
        // SAFETY: these are copied header flags on a private rooted output.
        unsafe {
            let raw = target.clone().as_raw();
            (*raw).sxpinfo.set_obj(source.header().sxpinfo.obj());
            (*raw).sxpinfo.set_gp(source.header().sxpinfo.gp());
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "safety_tests.rs"]
mod safety_tests;
