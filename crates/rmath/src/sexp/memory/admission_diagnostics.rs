#![forbid(unsafe_code)]
//! Test-only observations of the first real admission refusal. No roots or callbacks.

use super::RArena;

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub(crate) struct AdmissionRefusal {
    operation: &'static str,
    active: usize,
    committed_slots: usize,
    free: usize,
    max_nodes: usize,
    charged_bytes: usize,
    transient_bytes: usize,
    max_bytes: usize,
    requested_nodes: usize,
    requested_header_bytes: Option<usize>,
    requested_payload_bytes: Option<usize>,
    requested_workspace_bytes: Option<usize>,
    requested_total_charge: Option<usize>,
}

impl RArena {
    pub(super) fn note_admission_refusal(
        &self,
        operation: &'static str,
        requested_nodes: usize,
        header: Option<usize>,
        payload: Option<usize>,
        workspace: Option<usize>,
        total: Option<usize>,
    ) {
        if self.admission_refusal.get().is_some() {
            return;
        }
        self.admission_refusal.set(Some(AdmissionRefusal {
            operation,
            active: self.node_count(),
            committed_slots: self.backing.header_bytes.get() / super::NODE_BYTES,
            free: self.free_count(),
            max_nodes: self.budget.max_nodes,
            charged_bytes: self.allocated_bytes.get(),
            transient_bytes: self.transient_bytes.get(),
            max_bytes: self.budget.max_bytes,
            requested_nodes,
            requested_header_bytes: header,
            requested_payload_bytes: payload,
            requested_workspace_bytes: workspace,
            requested_total_charge: total,
        }));
    }

    pub(crate) fn admission_refusal(&self) -> Option<AdmissionRefusal> {
        self.admission_refusal.get()
    }

    pub(crate) fn clear_admission_refusal(&mut self) {
        self.admission_refusal.set(None);
    }
}

#[test]
fn refusal_observation_preserves_actual_node_and_byte_admission() {
    use crate::sexp::{SEXPTYPE, memory::ArenaBudget};
    let mut nodes = RArena::with_budget(ArenaBudget::new(0, 1));
    assert!(!nodes.alloc_node(SEXPTYPE::LISTSXP).is_null());
    assert!(nodes.alloc_node(SEXPTYPE::LISTSXP).is_null());
    let refusal = nodes.admission_refusal().unwrap();
    assert_eq!(
        (refusal.active, refusal.committed_slots, refusal.free),
        (1, 1, 0)
    );
    assert_eq!((refusal.operation, refusal.max_nodes), ("scalar node", 1));
    let mut bytes = RArena::with_budget(ArenaBudget::new(super::NODE_BYTES + 4, 0));
    assert!(bytes.alloc_vector(SEXPTYPE::INTSXP, 2).is_null());
    let refusal = bytes.admission_refusal().unwrap();
    assert_eq!(
        (refusal.active, refusal.committed_slots, refusal.free),
        (0, 0, 0)
    );
    assert_eq!(refusal.requested_header_bytes, Some(super::NODE_BYTES));
    assert_eq!(refusal.requested_payload_bytes, Some(8));
    assert_eq!(refusal.requested_total_charge, Some(super::NODE_BYTES + 8));
    bytes.clear_admission_refusal();
    assert!(bytes.try_reserve_transient(super::NODE_BYTES + 5).is_none());
    assert_eq!(
        bytes.admission_refusal().unwrap().requested_workspace_bytes,
        Some(super::NODE_BYTES + 5)
    );
}
