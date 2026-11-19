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
    pool: &Sexp<'static>,
    instruction_value: &Sexp<'static>,
) -> Result<
    (
        bool,
        std::collections::BTreeMap<usize, super::gnu_switch::OwnedSwitch>,
    ),
    String,
> {
    // Keep the original pool's owner available through proof and charge its
    // heap even when every entry is an immutable sentinel (or the pool is empty).
    let pin = pool.pin_runtime().map_err(|error| error.to_string())?;
    let code_pin = instruction_value
        .pin_runtime()
        .map_err(|error| error.to_string())?;
    let pool_node = pool.allocation().map_err(|error| error.to_string())?;
    if !pool_node.same_heap(
        instruction_value
            .allocation()
            .map_err(|error| error.to_string())?,
    ) {
        return Err("bytecode instructions and constants have different owners".into());
    }
    let ledger = pool_node.heap_identity().byte_ledger();
    let switches = super::gnu_switch::prepare_owned(code, constants)?;
    let targets = switches
        .iter()
        .map(|(pc, table)| (*pc, table.successors()))
        .collect();
    let denied = std::cell::Cell::new(false);
    let mut validation = validate_gnu_adapter_impl(
        code,
        constants.len(),
        Some(&targets),
        Some(&ledger),
        Some(&denied),
    );
    if validation.is_err() && denied.get() {
        // No proof buffer or R payload loan survives this callback. The
        // rooted original code, pool and entry snapshot retain their graph.
        let owner = crate::sexp::owner::StoredOwner::from_value(pool)
            .and_then(|owner| {
                owner
                    .managed()
                    .ok_or(crate::sexp::object::SexpError::RootUnavailable)
            })
            .map_err(|error| error.to_string())?;
        crate::sexp::owner::with_runtime(&owner, |access| {
            access.with_native(|owner| owner.full_gc())
        })
        .and_then(|result| result)
        .map_err(|error| error.to_string())?;
        if pool.typeof_() != crate::sexp::ffi::SEXPTYPE::VECSXP
            || pool.len() != constants.len() as i64
        {
            return Err("bytecode constant pool changed during validation collection".into());
        }
        for (index, value) in constants.iter().enumerate() {
            if pool
                .try_vector_elt(index as i64)
                .map_err(|error| error.to_string())?
                != *value
            {
                return Err("bytecode constant pool changed during validation collection".into());
            }
        }
        if instruction_value.typeof_() != crate::sexp::ffi::SEXPTYPE::INTSXP
            || instruction_value.len() != code.len() as i64
        {
            return Err("bytecode instructions changed during validation collection".into());
        }
        for (index, word) in code.iter().enumerate() {
            if instruction_value
                .try_integer_elt(index as i64)
                .map_err(|error| error.to_string())?
                != *word
            {
                return Err("bytecode instructions changed during validation collection".into());
            }
        }
        for (pc, table) in &switches {
            table.revalidate_snapshot(code, constants, *pc)?;
        }
        denied.set(false);
        validation = validate_gnu_adapter_impl(
            code,
            constants.len(),
            Some(&targets),
            Some(&ledger),
            Some(&denied),
        );
    }
    let valid = validation?;
    if let Some(pin) = &pin {
        pin.require_live().map_err(|error| error.to_string())?;
    }
    if let Some(pin) = &code_pin {
        pin.require_live().map_err(|error| error.to_string())?;
    }
    Ok((valid, switches))
}

mod consts;
mod interpreter;
mod validate;
mod validation_workspace;

pub use consts::*;
pub use interpreter::*;
pub use validate::*;

#[cfg(test)]
mod tests_a;
#[cfg(test)]
mod tests_b;
#[cfg(test)]
mod tests_util;
