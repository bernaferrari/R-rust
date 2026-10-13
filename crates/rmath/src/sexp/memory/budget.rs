//! Arena budget and thresholds.

use super::*;

/// Budget for arena allocations to prevent unbounded growth.
///
/// A budget of `0` means unlimited for that dimension.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ArenaBudget {
    /// Maximum accounted bytes allowed, including arena data and
    /// reservations for temporary native workspaces (0 = unlimited).
    pub max_bytes: usize,
    /// Maximum number of active nodes allowed in this arena (0 = unlimited).
    pub max_nodes: usize,
}

impl ArenaBudget {
    /// Create an unlimited budget.
    pub const fn unlimited() -> Self {
        ArenaBudget {
            max_bytes: 0,
            max_nodes: 0,
        }
    }

    /// Create a budget with the given limits.
    pub const fn new(max_bytes: usize, max_nodes: usize) -> Self {
        ArenaBudget {
            max_bytes,
            max_nodes,
        }
    }
}

// Each canonical slot retains its actual typed payload lease. Allocation
// identity is independent of native numeric projections.

// ---------------------------------------------------------------------------
// RArena: arena allocator for R objects
// ---------------------------------------------------------------------------

pub(super) fn empty_payload_shape(header: &SexprecCore) -> bool {
    if !header.payload.is_empty() || header.sxpinfo.type_of() == SEXPTYPE::CHARSXP {
        return false;
    }
    match header.data {
        NodeBody::Vector(vector) => {
            vector.length == 0 || (header.sxpinfo.alt() && vector.length >= 0)
        }
        _ => true,
    }
}
