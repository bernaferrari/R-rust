#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Side table for environment hash lookups.
//!
//! Uses a thread-local `HashMap` to provide O(1) lookups for environment bindings
//! without modifying the `#[repr(C)]` Envsxp struct or the GC's pointer-remapping logic.
//! The pairlist remains the authoritative storage; this hash table is an optimization layer.

use hashbrown::HashMap;
use std::cell::RefCell;

use super::ffi::SEXP;

/// Number of bindings in a frame before auto-promotion to hash table.
const PROMOTION_THRESHOLD: usize = 100;

thread_local! {
    /// Maps environment addresses to their hash tables (symbol address -> value SEXP).
    static ENV_HASH_TABLES: RefCell<HashMap<usize, HashMap<usize, SEXP>>> = RefCell::new(HashMap::new());
}

/// Check if an environment has an associated hash table.
pub fn env_has_hash_table(env: SEXP) -> bool {
    ENV_HASH_TABLES.with(|tables| tables.borrow().contains_key(&(env as usize)))
}

/// Look up a symbol in the environment's hash table.
///
/// Returns `None` if no hash table exists or the symbol is not found.
pub fn hash_get(env: SEXP, symbol: SEXP) -> Option<SEXP> {
    ENV_HASH_TABLES.with(|tables| {
        tables
            .borrow()
            .get(&(env as usize))?
            .get(&(symbol as usize))
            .copied()
    })
}

/// Insert a binding into the environment's hash table (if one exists).
pub fn hash_insert(env: SEXP, symbol: SEXP, value: SEXP) {
    ENV_HASH_TABLES.with(|tables| {
        if let Some(ht) = tables.borrow_mut().get_mut(&(env as usize)) {
            ht.insert(symbol as usize, value);
        }
    })
}

/// Remove a binding from the environment's hash table (if one exists).
pub fn hash_remove(env: SEXP, symbol: SEXP) {
    ENV_HASH_TABLES.with(|tables| {
        if let Some(ht) = tables.borrow_mut().get_mut(&(env as usize)) {
            ht.remove(&(symbol as usize));
        }
    })
}

/// Promote an environment to use a hash table by bulk-inserting all current bindings.
pub fn promote_to_hash_table(env: SEXP, bindings: &[(SEXP, SEXP)]) {
    ENV_HASH_TABLES.with(|tables| {
        let mut tables = tables.borrow_mut();
        let ht = tables
            .entry(env as usize)
            .or_insert_with(|| HashMap::with_capacity(bindings.len()));
        for (sym, val) in bindings {
            ht.insert(*sym as usize, *val);
        }
    });
}

/// Check whether a pairlist length exceeds the promotion threshold.
pub fn should_promote(pairlist_length: usize) -> bool {
    pairlist_length >= PROMOTION_THRESHOLD
}

/// Remove an environment's hash table entry (for cleanup).
pub fn remove_env(env: SEXP) {
    ENV_HASH_TABLES.with(|tables| {
        tables.borrow_mut().remove(&(env as usize));
    });
}
