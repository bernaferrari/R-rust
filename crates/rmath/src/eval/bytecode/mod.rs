//! R bytecode evaluation.
//!
//! R compiles expressions to bytecode for faster execution.
//! The bytecode format is a vector of integers where each
//! instruction is an opcode followed by operand indices.

// Private instruction execution owns each operand and publishes initialized
// scalar and call nodes through the original allocation-domain capability.
use std::{
    collections::HashSet,
    os::raw::{c_double, c_int},
};

use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::object::{NodeAllocator, NodeDomain, Sexp, SexpError};
use crate::sexp::owner::RuntimeAccess;

fn sexp_err(context: &str, err: SexpError) -> String {
    format!("{context}: {err}")
}

pub(crate) const GNU_BC_OPERAND_WIDTHS: [u8; GNU_BC_OPCODE_COUNT] = [
    0, 0, 1, 2, 0, 0, 0, 2, 1, 0, 0, 3, 1, 0, 0, 0, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 1,
    0, 0, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 2,
    0, 2, 0, 2, 0, 2, 0, 2, 0, 2, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 1, 2, 1, 1, 1, 0, 1,
    1, 1, 2, 1, 0, 0, 4, 0, 2, 2, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 1, 1, 2, 2, 1, 1, 1, 2, 0, 0, 1, 0,
    0,
];

/// Validate against the exact owning constants and frozen branch tables used
/// by execution. A callback cannot substitute a different mutable pool here.
pub(crate) fn validate_gnu_adapter_with_owned_constants(
    code: &[c_int],
    constants: &[Sexp<'static>],
) -> Result<
    (
        bool,
        std::collections::BTreeMap<usize, super::gnu_switch::OwnedSwitch>,
    ),
    String,
> {
    let switches = super::gnu_switch::prepare_owned(code, constants)?;
    let targets = switches
        .iter()
        .map(|(pc, table)| (*pc, table.successors()))
        .collect();
    let valid = validate_gnu_adapter_impl(code, constants.len(), Some(&targets))?;
    Ok((valid, switches))
}

mod consts;
mod interpreter;
mod validate;

pub use consts::*;
pub use interpreter::*;
pub use validate::*;

#[cfg(test)]
mod tests_a;
#[cfg(test)]
mod tests_b;
#[cfg(test)]
mod tests_util;
