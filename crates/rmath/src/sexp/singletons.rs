#![forbid(unsafe_code)]
//! Process-lifetime owners for immutable compatibility projections.
//!
//! Every byte that legacy code may project has interior storage. Atomics give
//! these process globals ordinary Rust Send/Sync without asserting those
//! properties for SexprecCore. Legacy reads remain an audited layout bridge;
//! mutators must reject these shared values before writing.

use super::super::ffi::{SEXP, SEXPTYPE, SexprecCore, SexprecData, SxpInfo, Vecsxp};
use std::{
    mem::{ManuallyDrop, align_of, offset_of, size_of},
    ptr,
    sync::{
        OnceLock,
        atomic::{AtomicI32, AtomicPtr, AtomicU8, AtomicU16, AtomicU32},
    },
};

#[repr(C)]
struct SharedInfo {
    flags: AtomicU32,
    count: AtomicU8,
    pad: AtomicU8,
    pad2: AtomicU16,
}

#[repr(C)]
union SharedData {
    bytes: ManuallyDrop<[AtomicU8; size_of::<SexprecData>()]>,
    // Preserve the native union's alignment on both 32- and 64-bit targets.
    vector_alignment: Vecsxp,
}

#[repr(C)]
struct Singleton {
    info: SharedInfo,
    attrib: AtomicPtr<SexprecCore>,
    next: AtomicPtr<SexprecCore>,
    previous: AtomicPtr<SexprecCore>,
    data: SharedData,
}

const _: () = {
    assert!(size_of::<SharedInfo>() == size_of::<SxpInfo>());
    assert!(align_of::<SharedInfo>() == align_of::<SxpInfo>());
    assert!(offset_of!(SharedInfo, flags) == offset_of!(SxpInfo, type_and_flags));
    assert!(offset_of!(SharedInfo, count) == offset_of!(SxpInfo, rcount));
    assert!(offset_of!(SharedInfo, pad) == offset_of!(SxpInfo, _pad));
    assert!(offset_of!(SharedInfo, pad2) == offset_of!(SxpInfo, _pad2));
    assert!(size_of::<SharedData>() == size_of::<SexprecData>());
    assert!(align_of::<SharedData>() == align_of::<SexprecData>());
    assert!(size_of::<Singleton>() == size_of::<SexprecCore>());
    assert!(align_of::<Singleton>() == align_of::<SexprecCore>());
    assert!(offset_of!(Singleton, info) == offset_of!(SexprecCore, sxpinfo));
    assert!(offset_of!(Singleton, attrib) == offset_of!(SexprecCore, attrib));
    assert!(offset_of!(Singleton, next) == offset_of!(SexprecCore, gengc_next_node));
    assert!(offset_of!(Singleton, previous) == offset_of!(SexprecCore, gengc_prev_node));
    assert!(offset_of!(Singleton, data) == offset_of!(SexprecCore, data));
};

impl Singleton {
    fn new(kind: SEXPTYPE, marked: bool, na: bool, logical: Option<&'static AtomicI32>) -> Self {
        let mut info = SxpInfo::new(kind);
        info.set_mark(marked);
        info.set_named(2);
        info.set_gp(u16::from(na));
        info.set_scalar(logical.is_some());
        let mut bytes = [0_u8; size_of::<SexprecData>()];
        if logical.is_some() {
            let one = 1_i64.to_ne_bytes();
            let length_offset = offset_of!(Vecsxp, length);
            bytes[length_offset..length_offset + one.len()].copy_from_slice(&one);
            let true_offset = offset_of!(Vecsxp, truelength);
            bytes[true_offset..true_offset + one.len()].copy_from_slice(&one);
        }
        Self {
            info: SharedInfo {
                flags: AtomicU32::new(info.type_and_flags),
                count: AtomicU8::new(info.rcount),
                pad: AtomicU8::new(info._pad),
                pad2: AtomicU16::new(info._pad2),
            },
            attrib: AtomicPtr::new(ptr::null_mut()),
            next: AtomicPtr::new(logical.map_or(ptr::null_mut(), |value| value.as_ptr().cast())),
            previous: AtomicPtr::new(ptr::null_mut()),
            data: SharedData {
                bytes: ManuallyDrop::new(bytes.map(AtomicU8::new)),
            },
        }
    }
    fn projection(&self) -> SEXP {
        ptr::from_ref(self).cast_mut().cast()
    }
}

static NIL: OnceLock<Singleton> = OnceLock::new();
static UNBOUND: OnceLock<Singleton> = OnceLock::new();
static MISSING: OnceLock<Singleton> = OnceLock::new();
static RESTART: OnceLock<Singleton> = OnceLock::new();
static TRUE: OnceLock<Singleton> = OnceLock::new();
static FALSE: OnceLock<Singleton> = OnceLock::new();
static NA_STRING: OnceLock<Singleton> = OnceLock::new();
static TRUE_DATA: AtomicI32 = AtomicI32::new(1);
static FALSE_DATA: AtomicI32 = AtomicI32::new(0);

pub(super) fn nil() -> SEXP {
    NIL.get_or_init(|| Singleton::new(SEXPTYPE::NILSXP, false, false, None))
        .projection()
}
pub(super) fn unbound() -> SEXP {
    UNBOUND
        .get_or_init(|| Singleton::new(SEXPTYPE::SYMSXP, true, false, None))
        .projection()
}
pub(super) fn missing() -> SEXP {
    MISSING
        .get_or_init(|| Singleton::new(SEXPTYPE::SYMSXP, true, false, None))
        .projection()
}
pub(super) fn restart() -> SEXP {
    RESTART
        .get_or_init(|| Singleton::new(SEXPTYPE::SPECIALSXP, true, false, None))
        .projection()
}
pub(super) fn logical(value: bool) -> SEXP {
    let (slot, data) = if value {
        (&TRUE, &TRUE_DATA)
    } else {
        (&FALSE, &FALSE_DATA)
    };
    slot.get_or_init(|| Singleton::new(SEXPTYPE::LGLSXP, false, false, Some(data)))
        .projection()
}
pub(super) fn na_string() -> SEXP {
    NA_STRING
        .get_or_init(|| Singleton::new(SEXPTYPE::CHARSXP, false, true, None))
        .projection()
}

/// Address comparison never reads the candidate and never initializes a new
/// singleton. Return the owner's pointer, preserving its actual provenance.
pub(super) fn canonical_projection(candidate: SEXP) -> Option<SEXP> {
    [
        &NIL, &UNBOUND, &MISSING, &RESTART, &TRUE, &FALSE, &NA_STRING,
    ]
    .into_iter()
    .filter_map(OnceLock::get)
    .map(Singleton::projection)
    .find(|pointer| ptr::eq(*pointer, candidate))
}
