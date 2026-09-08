#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(unused_imports)]

//! Port of R's src/main/subassign.c
//!
//! Subset mutation for lists and vectors: the `[<-`, `[[<-`, and `$<-` operators.
//!
//! Ported internal helpers:
//!   getNames, EnlargeVector, EnlargeNames, embedInVector, dispatch_asvector,
//!   SubassignTypeFix, gi, DeleteListElements, VECTOR_ELT_FIX_NAMED,
//!   VectorAssign, MatrixAssign, ArrayAssign, GetOneIndex, SimpleListAssign,
//!   listRemove, DeleteOneVectorListItem, SubAssignArgs, R_DispatchOrEvalSP,
//!   errorNotSubsettable, errorMissingSubscript, errorOutOfBoundsSEXP
//!
//! Ported exported functions:
//!   do_subassign, do_subassign_dflt, do_subassign2, do_subassign2_dflt,
//!   do_subassign3, R_subassign3_dflt, SubassignTypeSym, SubassignDotsNames,
//!   GetSubassignSxpVec, var_assign

//! Split into domain submodules; every public path `crate::mainutils::subassign::*`
//! resolves exactly as before via the glob re-exports below.

mod api;
mod array;
mod assign;
mod dispatch;
mod listops;
mod matrix;
mod subassign3;
mod support;
#[cfg(test)]
mod tests;
mod vector;

pub use self::api::*;
pub use self::array::*;
pub use self::assign::*;
pub use self::dispatch::*;
pub use self::listops::*;
pub use self::matrix::*;
pub use self::subassign3::*;
pub use self::support::*;
pub use self::vector::*;
