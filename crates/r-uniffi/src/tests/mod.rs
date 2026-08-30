//! In-crate tests: shared support plus per-area test modules. These live
//! inside the crate so they can exercise `pub(crate)` seams (injectable
//! worker init, `enqueue`, `request_with_timeout`, `OperationTable`).

mod operation_tests;
mod reliability_tests;
mod session_tests;
pub(crate) mod support;
