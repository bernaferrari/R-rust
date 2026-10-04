#![forbid(unsafe_code)]
//! Owned declarations; an erased address never determines its interface.
use crate::unix::dynload::DL_FUNC;
use std::ffi::{CStr, CString};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Interface {
    C,
    Fortran,
    Call,
    External,
}

impl Interface {
    pub(super) fn from_native(kind: i32) -> Option<Self> {
        match kind {
            1 => Some(Self::C),
            2 => Some(Self::Fortran),
            3 => Some(Self::Call),
            4 => Some(Self::External),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Arity {
    Fixed(usize),
    Variadic,
}

impl Arity {
    pub(super) fn from_native(count: i32) -> Self {
        usize::try_from(count).map_or(Self::Variadic, Self::Fixed)
    }
}

#[derive(Clone)]
pub(super) struct Routine {
    pub(super) name: CString,
    pub(super) function: DL_FUNC,
    pub(super) interface: Interface,
    pub(super) arity: Arity,
    pub(super) types: Option<Vec<i32>>,
}

impl Routine {
    pub(super) fn validate(&self, interface: i32, count: usize) -> Result<(), String> {
        if Interface::from_native(interface) != Some(self.interface) {
            return Err(format!(
                "incorrect native interface for '{}'",
                self.name.to_string_lossy()
            ));
        }
        if let Arity::Fixed(expected) = self.arity {
            if expected != count {
                return Err(format!(
                    "Incorrect number of arguments ({count}), expecting {expected} for '{}'",
                    self.name.to_string_lossy()
                ));
            }
        }
        Ok(())
    }
}

#[derive(Default)]
pub(super) struct Registry {
    entries: Vec<Routine>,
}

impl Registry {
    pub(super) fn replace(&mut self, interface: Interface, entries: Vec<Routine>) {
        self.entries.retain(|entry| entry.interface != interface);
        self.entries.extend(entries);
    }

    pub(super) fn lookup(&self, name: &CStr, kind: i32) -> Option<Routine> {
        // GNU's ANY order is C, Call, Fortran, External, regardless of the
        // order in which tables were replaced.
        [
            Interface::C,
            Interface::Call,
            Interface::Fortran,
            Interface::External,
        ]
        .into_iter()
        .filter(|interface| kind == -1 || Interface::from_native(kind) == Some(*interface))
        .find_map(|interface| {
            self.entries
                .iter()
                .find(|entry| entry.interface == interface && entry.name.as_c_str() == name)
                .cloned()
        })
    }
}
