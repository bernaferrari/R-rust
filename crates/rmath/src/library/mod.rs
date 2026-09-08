//! R library packages

#![allow(unused_doc_comments)]
#![allow(unused_attributes)]
// Minimal per-package lint allowances: the C-transliterated package ports
// still trip exactly these rustc lints (verified per package with
// `cargo clippy -p rmath --all-targets`); everything else is lint-clean.
#[allow(unused_imports, unused_variables, unused_mut, unused_assignments)]
pub mod graphics;
#[allow(unused_imports, unused_variables, unused_mut, unused_assignments)]
pub mod grdevices;
#[allow(unused_imports, unused_variables, unused_mut, unused_assignments)]
pub mod grid;
#[allow(unused_imports)]
pub mod methods;
#[allow(unused_imports, unused_assignments)]
pub mod parallel;
pub mod splines;
#[allow(unused_imports, unused_variables, unused_mut, unused_assignments)]
pub mod stats;
#[cfg(not(target_os = "android"))]
#[allow(unused_imports)]
pub mod tcltk;
#[allow(unused_imports, unused_mut, unused_assignments)]
pub mod tools;
#[allow(unused_imports, unused_variables, unused_mut, unused_assignments)]
pub mod utils;
